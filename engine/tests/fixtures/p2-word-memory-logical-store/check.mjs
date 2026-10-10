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
  if (word && [33, 9, 49, 139].includes(opcode)) {
    const modrm = take(), field = Math.floor(modrm / 8) % 8, memory = operand(modrm);
    assert.equal(memory.register, undefined, 'authored memory operand only');
    return finish(opcode === 139 ? {kind:'load-word',destination:field,memory} : {kind:'logical',operation:{33:'and',9:'or',49:'xor'}[opcode],source:field,memory});
  }
  if (word && opcode >= 184 && opcode <= 191) return finish({kind: 'move', destination: opcode - 184, value: little(2)});
  if (word && opcode === 131) {
    const modrm = take(), field = Math.floor(modrm / 8) % 8;
    assert.ok(modrm >= 192 && [0, 2, 3, 5].includes(field), 'authored register carry producer/consumer only');
    const n = signedByte(); return finish({kind: 'register-arithmetic', subtract: field === 3 || field === 5, consumeCarry: field === 2 || field === 3, destination: modrm % 8, value: (n + 65536) % 65536});
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
function storePacket(c,address,value,foreign) {
  if(foreign){Buffer.from(foreign.packet_hex,'hex').copy(c.arena,100);return foreign.status;}
  const fault=c.ram.access(address,2,2);
  if(fault){record('R3MH',40,[1,0,fault.detail,fault.address,2,2],4).copy(c.arena,100);return 0;}
  const bytes=Buffer.alloc(2);bytes.writeUInt16LE(value);c.ram.write(address,bytes);
  record('R3MH',40,[0,0,0,0,0,2],4).copy(c.arena,100);
  return c.ram.current(c.active?.tokens??new Map())?0:11;
}
function validateStore(arena,address,status) {
  if(![0,11].includes(status))return {reason:7,detail:3};
  const b=arena.subarray(100,140),[result,value,detail,at,access,length]=Array.from({length:6},(_,i)=>b.readUInt32LE(16+4*i));
  if(!b.subarray(0,16).equals(record('R3MH',40,[],4).subarray(0,16))||value||length!==2)return {reason:7,detail:2};
  if(result===0)return !detail&&!at&&!access&&address<=U32-2?{}:{reason:7,detail:2};
  if(status===11)return {reason:7,detail:2};
  if(result===1){const valid=access===2&&detail>=1&&detail<=3&&(detail===3?address>U32-2&&at===address:address<=U32-2&&at>=address&&at<address+2);return valid?{reason:5,detail,address:at,access:2,length:2}:{reason:7,detail:2};}
  if(result===2&&!at&&!access&&[1,2].includes(detail))return {reason:7,detail:detail===1?1:3};
  return {reason:7,detail:2};
}
export function wordLogical(kind, left, right, oldFlags) {
  uint(left, 65535); uint(right, 65535);
  assert.ok(['and', 'or', 'xor'].includes(kind));
  const result = kind === 'and' ? left & right : kind === 'or' ? left | right : left ^ right;
  let odd = 0; for (let n = result % 256; n; n = Math.floor(n / 2)) odd ^= n % 2;
  const flags = (oldFlags & 1024) + 2 + Number(!odd) * 4 + Number(result === 0) * 64 + Number(result >= 32768) * 128;
  return {result, flags};
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
  let invalidated = false;
  if (op.kind === 'logical' || op.kind === 'load-word') {
    const address = addressOf(op.memory, old);
    if (foreign?.phase === 'read') addresses.push(address);
    const checked = validateNarrow(c.arena, address, narrowPacket(c, address, foreign?.phase === 'read' ? foreign : null));
    if (checked.reason) return {fault: checked};
    if (op.kind === 'load-word') registers[op.destination] = old[op.destination] - old[op.destination] % 65536 + checked.value;
    else {
      const out = wordLogical(op.operation, checked.value, old[op.source] % 65536, flags);
      if (foreign?.phase === 'store') addresses.push(address);
      const status = storePacket(c, address, out.result, foreign?.phase === 'store' ? foreign : null);
      const checkedStore = validateStore(c.arena, address, status);
      if (checkedStore.reason) return {fault:checkedStore};
      flags = out.flags; invalidated = status === 11;
    }
  } else if (op.kind === 'register-arithmetic' || op.kind === 'move') {
    const out = op.kind === 'register-arithmetic' ? wordArithmetic(old[op.destination] % 65536, op.value, flags, op.subtract, op.consumeCarry ? flags % 2 : 0) : {result: op.value, flags};
    flags = out.flags; registers[op.destination] = old[op.destination] - old[op.destination] % 65536 + out.result;
  } else if (op.kind === 'set') {
    const parent = op.alias % 4, place = op.alias < 4 ? 1 : 256;
    registers[parent] += (Number(predicate(op.cc, flags)) - Math.floor(old[parent] / place) % 256) * place;
  } else if (op.kind === 'branch') {if (op.cc === null || predicate(op.cc, flags)) pc = op.target;}
  else assert.equal(op.kind, 'nop');
  return {cpu: {registers, flags, pc}, invalidated};
}
function childRun(c, unit, budget) {
  let cpu = state(c.arena), retired = 0, outcome; const addresses = []; c.active = unit;
  for (;;) {
    if (c.arena.readUInt32LE(96)) {outcome = {reason: 2}; break;}
    if (retired === budget) {outcome = {reason: 1}; break;}
    const op = unit.instructions.get(cpu.pc); if (!op) {outcome = {reason: 3}; break;}
    const step = execute(c, op, cpu, unit.foreign, addresses); if (step.fault) {outcome = step.fault; break;}
    cpu = step.cpu; retired++; if (step.invalidated) {outcome={reason:6};break;}
  }
  record('R3ST', 56, [...cpu.registers, cpu.pc, cpu.flags]).copy(c.arena);
  record('R3EX', 40, [outcome.reason, retired, outcome.detail ?? 0, outcome.address ?? 0, outcome.access ?? 0, outcome.length ?? 0], [...unit.instructions.values()].some(op=>['logical','load-word'].includes(op.kind))?2:1).copy(c.arena, 56);
  return {...outcome, retired, addresses};
}
function installation(arena, unit) {record('R3IN', 32, [unit.id_low, unit.id_high, unit.slot, 0]).copy(arena, 140);}
export function generated(c, action) {
  if (c.closed) return {status: 5, retired: 0, addresses: []};
  if (action.channel === 'direct') {const unit = c.units.get(action.group); assert.ok(unit); if (!c.ram.current(unit.tokens)) return {status: 4, retired: 0, addresses: []}; return {status: 0, ...childRun(c, unit, action.budget)};}
  let retired = 0; const addresses = [];
  for (;;) {
    if (c.arena.readUInt32LE(96) || retired === action.budget) {const reason = c.arena.readUInt32LE(96) ? 2 : 1; record('R3EX', 40, [reason, retired, 0, 0, 0, 0], 3).copy(c.arena, 56); return {status: 0, retired, reason, addresses};}
    const matches = [...c.units.values()].filter(u => u.installed && u.instructions.has(c.arena.readUInt32LE(48)));
    const unit = matches.find(u => c.ram.current(u.tokens));
    if (!unit) {
      if (matches.length) {if(retired)c.arena.writeUInt32LE(retired,76);return {status:4,retired,addresses};}
      record('R3EX',40,[3,retired,0,0,0,0],3).copy(c.arena,56);return {status:0,retired,reason:3,addresses};
    }
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
  const blocks = roots.map((pc,i) => {
    if (!entries) return [pc,c.arena.readUInt32LE(144+i*8)];
    const end=Math.min((pc-pc%4096)+4096,...roots.filter(p=>p>pc));let length=0;
    for (;;) {assert.ok(pc+length<end);const raw=c.ram.read(pc+length,Math.min(15,end-pc-length),4);assert.ok(raw.bytes);const op=decodeAt(raw.bytes,0,pc+length);length+=op.length;assert.ok(pc+length<=end);if(op.kind==='branch')break;}
    return [pc,length];
  });
  const instructions = new Map();
  for (const [i, [pc, length]] of blocks.entries()) {
    assert.ok(length > 0 && pc + length <= U32); for (const [other, n] of blocks.slice(0, i)) assert.ok(pc + length <= other || other + n <= pc);
    const raw=c.ram.read(pc,length,4);assert.ok(raw.bytes);let at = 0; while (at < length) {assert.ok(instructions.size < 64); const op = decodeAt(raw.bytes, at, pc + at); at += op.length; assert.ok(at <= length); if (op.kind === 'branch') assert.equal(at, length); assert.ok(!instructions.has(op.pc)); instructions.set(op.pc, op);}
  }
  return {blocks, instructions, tokens: c.ram.snapshot(blocks)};
}
function safeFile(directory, file) {
  assert.equal(typeof file, 'string'); assert.equal(isAbsolute(file), false);
  assert.ok(file && !file.split('/').includes('..'), 'relative literal file key');
  const path = resolve(directory, file), difference = relative(directory, path);
  assert.ok(difference && !difference.startsWith('..') && !isAbsolute(difference));
  assert.equal(difference, file, 'canonical relative file key');
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

function moduleInterface(bytes,owner,dispatcher,group) {
  const p=wasmInterface(bytes);assert.deepEqual(p.sections,[1,2,3,7,10]);
  const memory=!dispatcher&&['main','alias','control','helper','changed','equal'].includes(group);
  const imports=[{module:'env',name:'memory',kind:'memory',flags:0,minimum:1}];
  if(dispatcher)imports.push({module:'env',name:'table',kind:'table',element:112,flags:1,minimum:8,maximum:8},{module:'ring3',name:'guard_dispatch_entry',kind:'function',type:1},{module:'ring3',name:'find_installed_resident',kind:'function',type:2});
  else{imports.push({module:'ring3',name:owner==='resident'?'guard_resident':'guard',kind:'function',type:1});if(memory)imports.push({module:'ring3',name:'read16',kind:'function',type:2},{module:'ring3',name:owner==='resident'?'store_resident16':'store16',kind:'function',type:3});}
  assert.deepEqual(p.imports,imports);assert.deepEqual(p.exports.map(({name,kind})=>({name,kind})),[{name:'run',kind:'function'}]);
  const functions=dispatcher?[1,2,0]:memory?[1,2,3,0]:[1,0];assert.deepEqual(p.functions,functions);assert.equal(p.types.length,dispatcher?3:memory?4:2);assert.equal(p.bodies.length,1);assert.equal(p.exports[0].index,functions.length-1);
  assert.deepEqual(p.types[0],{parameters:Array(4).fill(127),results:[127]});
  for(const f of imports.filter(r=>r.kind==='function')){const n=f.name==='guard'?6:f.name==='guard_resident'?7:f.name==='guard_dispatch_entry'?5:f.name==='read16'?1:f.name==='store16'?2:f.name==='store_resident16'?6:3;assert.deepEqual(p.types[f.type],{parameters:Array(n).fill(127),results:[127]});}
  return{imports:imports.map(({module,name,kind})=>({module,name,kind})),exports:p.exports.map(({name,kind})=>({name,kind}))};
}

function moduleBinding(bytes, row, spec) {
  const body = wasmInterface(bytes).bodies[0]; let at = 0;
  const byte = () => {assert.ok(at < body.length); return body[at++];};
  const unsigned = () => {let value = 0, scale = 1; for(let i=0;i<5;i++){const n=byte();value+=(n%128)*scale;if(n<128)return uint(value);scale*=128;}assert.fail('bounded body ULEB');};
  const signed = () => {let value=0n,shift=0n,n;do{assert.ok(shift<35n);n=byte();value|=BigInt(n&127)<<shift;shift+=7n;}while(n&128);if(n&64)value-=1n<<shift;return Number(BigInt.asUintN(32,value));};
  const locals=Array.from({length:unsigned()},()=>[unsigned(),byte()]),dispatch=row.target==='dispatcher';
  const memory=['main','alias','control','helper','changed','equal'].includes(row.group);assert.deepEqual(locals,dispatch?[[5,0x7f]]:memory?[[16,0x7f],[1,0x7e],[6,0x7f]]:[[16,0x7f],[1,0x7e]]);
  const constants=dispatch?spec.key:row.owner==='replacement'?[...spec.key,row.generation]:[...spec.key,row.id_low,row.id_high];
  for(const value of constants){assert.equal(byte(),0x41);assert.equal(signed(),value);}
  for(const parameter of [0,1,3]){assert.equal(byte(),0x20);assert.equal(unsigned(),parameter);}
  assert.equal(byte(),0x10);assert.equal(unsigned(),0);assert.equal(byte(),0x22);assert.equal(unsigned(),dispatch?5:16);
  assert.deepEqual(Array.from({length:7},byte),[0x04,0x40,0x20,dispatch?5:16,0x0f,0x0b,dispatch?0x41:0x3f]);
}


const edgeLeft=[[0,65535,32768,32767,255,43690,4660,65535],[0,0,32767,32768,254,43690,4660,65535],[0,65535,32768,32767,255,43690,4660,65535]];
const edgeRight=[[65535,0,65535,65535,255,21845,3855,65535],[0,1,32768,0,1,21845,240,0],[0,65535,0,65535,1,21845,60875,0]];
function literalBank() {
  const b=Buffer.alloc(4096,204);let at=0;
  const emit=(opcode,source,tail)=>{const raw=[102,opcode,tail[0]+8*source,...tail.slice(1)];b.set(raw,at);at+=raw.length;};
  for(let family=0;family<3;family++)for(let r=0;r<8;r++)emit([33,9,49][family],r,[5,2*(family*8+r),64,0,0]);
  for(const opcode of[33,9,49])for(const[r,tail]of[[3,[131,128,81,0,0]],[6,[4,245,128,65,0,0]],[1,[132,11,128,65,0,0]],[0,[132,48,128,65,0,0]],[5,[132,61,128,65,0,0]],[2,[4,149,128,65,0,0]],[3,[67,255]],[4,[68,36,255]]])emit(opcode,r,tail);
  assert.equal(at,336);b.set([235,254],at);at=512;
  for(const opcode of[33,9,49]){
    for(let r=0;r<8;r++)emit(opcode,r,r===4?[4,36]:r===5?[69,0]:[r]);
    for(let r=0;r<8;r++)if(r!==4)emit(opcode,r,[4,r*8+5,128,65,0,0]);
    emit(opcode,3,[4,27]);
  }
  assert.equal(at,770);b.set([235,254],at);at=1536;
  for(const opcode of[33,9,49]){b[at++]=144;emit(opcode,0,[3]);}b.set([235,254],at);
  for(const[i,hex]of['66b8000066bfffff6683ef01662103668b0b0f92c30f90c20f94c474020f0b6683d7000f92c7eb00','66b8ffff66bfffff6683ef01663103668b0b0f92c30f90c20f94c475020f0b6683d7000f92c7eb00'].entries()){const raw=Buffer.from(hex,'hex');assert.equal(raw.length,40);b.set(raw,2048+64*i);}
  for(const[i,hex]of['660903668b1b6631130f94c3662133eb00','662103668b1b6631130f94c3660933eb00'].entries()){const raw=Buffer.from(hex,'hex');assert.equal(raw.length,17);b.set(raw,2560+64*i);}
  b.set(Buffer.from('663105001d000090eb00','hex'),3328);b.set(Buffer.from('660905401d000090eb00','hex'),3392);b.set([144,235,0],3584);b.set([15,11],3840);return b;
}
function literalHelpers(){return[{id:'read-width',phase:'read',status:0,packet_hex:record('R3MH',40,[0,32769,0,0,0,1],2).toString('hex')},{id:'store-width',phase:'store',status:0,packet_hex:record('R3MH',40,[0,0,0,0,0,1],4).toString('hex')}];}
function literalData(){const b=Buffer.from(Array.from({length:4096},(_,i)=>(73*i+11)%256));edgeLeft.flat().forEach((n,i)=>b.writeUInt16LE(n,2*i));for(const[at,n]of[[256,16640],[258,3855],[384,32769],[512,16896],[514,16898],[768,65535],[770,32768]])b.writeUInt16LE(n,at);b.set([144,235,0],3584);return b;}
const literalGroups=[
  {id:'main',blocks:[[4096,338]],instructions:49},{id:'alias',blocks:[[4608,260]],instructions:49},
  {id:'control',blocks:[[5632,14],[6144,29],[6175,9],[6208,29],[6239,9],[6656,17],[6720,17]],instructions:43},
  {id:'helper',blocks:[[5632,4]],instructions:2},
  {id:'changed',blocks:[[7424,10]],instructions:3},{id:'changed-successor',blocks:[[7431,3]],instructions:2},
  {id:'equal',blocks:[[7488,10]],instructions:3},{id:'equal-successor',blocks:[[7495,3]],instructions:2},
  {id:'other-code',blocks:[[7680,3]],instructions:2},{id:'unrelated',blocks:[[19968,3]],instructions:2}
];
function literalRows(){
  const bytes=literalBank(),rows=[];
  const row=(axis,pc,fields={})=>{const op=decodeAt(bytes,pc-CODE,pc);assert.equal(op.kind,'logical');rows.push({id:rows.length,axis,kind:op.operation,source:op.source,pc,length:op.length,hex:bytes.subarray(pc-CODE,op.next-CODE).toString('hex'),overrides:{},...fields});return op.next;};
  let pc=CODE;for(let k=0;k<3;k++)for(let n=0;n<8;n++)pc=row('edge',pc,{case:n,memory:edgeLeft[k][n],right:edgeRight[k][n]});
  for(let k=0;k<3;k++)for(const overrides of[{3:4294963200},{6:536870912},{3:2147483648,1:2147483648},{0:4294967295,6:1},{5:1,7:4294967295},{2:1073741824},{3:16769},{4:16769}])pc=row('wrap',pc,{overrides});
  pc=4608;for(let k=0;k<3;k++){for(let r=0;r<8;r++)pc=row('alias',pc,{alias:'base',overrides:{[r]:16768}});for(let r=0;r<8;r++)if(r!==4)pc=row('alias',pc,{alias:'index',overrides:{[r]:0}});pc=row('alias',pc,{alias:'both',overrides:{3:8384}});}
  for(const pc of[5633,5637,5641])row('fault',pc);assert.equal(rows.length,99);return rows;
}
function literalDescription(){const rows=literalRows();return{hex:literalBank().toString('hex'),forms:rows,fault_forms:rows.slice(-3).map((r,i)=>({...r,prefix:5632+i*4})),consumer:{paths:[0,1].map(i=>({id:i,pc:6144+64*i,arithmetic_pc:6156+64*i,end_pc:6184+64*i,instructions:12})),next_paths:[0,1].map(i=>({id:i,pc:6656+64*i,end_pc:6673+64*i,instructions:6}))},smc:[{id:'changed',pc:7424,next_pc:7431,end_pc:7434},{id:'equal',pc:7488,next_pc:7495,end_pc:7498}],invalid_pc:7936,groups:literalGroups};}
const seedLows=[[0,1,32767,65535,32768,4660,43690,21845],[32767,65535,0,32768,1,43690,21845,4660],[32768,32767,65535,32767,4660,1,21845,43690]];
function literalSeed(pc,flags=0xcd7,overrides={},pattern=0){const registers=seedLows[pattern].map((low,i)=>((0xa101+pattern*0x111+i*0x101)*65536+low)%U32);for(const[k,v]of Object.entries(overrides))registers[Number(k)]=uint(v>>>0);const b=Buffer.alloc(140);record('R3ST',56,[...registers,pc,flags]).copy(b);record('R3EX',40,[3,0,0,0,0,0],3).copy(b,56);record('R3MH',40,[0,0xdecafbad,0,0,0,0]).copy(b,100);return b.toString('hex');}
function littleWord(n){const b=Buffer.alloc(4);b.writeUInt32LE(n);return [...b];}
function literalSchedule(){
  const bank=literalDescription(),data=literalData().toString('hex'),groups=new Map(bank.groups.map(g=>[g.id,g])),actions=[],contexts=[];let next=0;
  function context(owner,entries,role='normal') {
    const spec={id:contexts.length+1,owner,entries,role,key:[0x574d0000+contexts.length+1,0x4c53544f],pages:6};contexts.push(spec);let diagnostic=0;
    const add=(kind,fields={})=>actions.push({id:next++,context:spec.id,kind,...fields});
    const input=(offset,hex,label)=>add('input',{offset,hex,label});const host=(name,args,fields={})=>add('host',{name,args,...fields});
    const call=(group,budget,label,retired,fields={})=>add('call',{group,channel:owner==='resident'&&!entries?'dispatch':'direct',budget,label,planned_retired:retired,...fields});
    const seed=(pc,flags=0xcd7,overrides={},pattern=0,label='CPU seed')=>input(0,literalSeed(pc,flags,overrides,pattern),label);
    const upload=(address,hex,label)=>{input(140,hex,label);host('upload',[address,hex.length/2]);};
    const diagnose=(address,count,phase)=>add('data',{address,count,file:`${spec.id}-${diagnostic++}-${phase}.bin`,label:count===1024?'complete readable RAM page':'bounded readable watched word'});
    const watch=address=>{diagnose(address-address%4,1,'watch');if(address%4===3)diagnose(address+1,1,'watch-second');};
    const resetWord=(address,n)=>{host('write8',[address,n&255]);host('write8',[address+1,n>>8]);};
    function compile(group,slot,file=group) {
      const g=groups.get(group),request=Buffer.alloc(g.blocks.length*(entries?4:8));
      g.blocks.forEach(([pc,n],i)=>{request.writeUInt32LE(pc,i*(entries?4:8));if(!entries)request.writeUInt32LE(n,i*8+4);});
      input(140,request.toString('hex'),'compiler descriptors');host(owner==='resident'?entries?'compile_resident_entries':'compile_resident':entries?'compile_entries':'compile',entries?[g.blocks.length,0]:[g.blocks.length],{group});
      add('module',{target:'child',group,slot,file:`${spec.id}-${file}.wasm`,helper_case:null});
      if(owner==='resident'){add('table',{group,slot});host('acknowledge_resident_installation',['key_low','key_high','unit_low','unit_high',slot],{group});if(slot===0){host('dispatcher_module',['key_low','key_high']);add('module',{target:'dispatcher',group:'dispatcher',slot:null,file:`${spec.id}-dispatcher.wasm`,helper_case:null});}}
    }
    function formSeed(f,flags,pattern=0) {const overrides={...f.overrides};if(f.axis==='edge')overrides[f.source]=(((0xb101+pattern*0x111+f.source*0x101)<<16)|f.right)>>>0;seed(f.pc,flags,overrides,pattern,'form and source seed');}
    function consumerSeed(path) {resetWord(17152+2*path.id,path.id?0x8000:0xffff);seed(path.pc,0xcd7,{3:17152+2*path.id,7:0xa808ffff});}
    function nextSeed(path) {upload(DATA,data,'independent current-address initial data');seed(path.pc,0xcd7,{0:path.id?0xa101fffd:0xa1010002,2:path.id?0xa3034200:0xa3030f0f,3:path.id?0x4202:0x4100,6:path.id?0xa7070001:0xa70700ff});}
    add('open');input(0,authoredArena().toString('hex'),'full authored nondefault arena');host('map',[CODE,1,7]);upload(CODE,bank.hex,'complete literal bank');if(role!=='smc')host('protect',[CODE,1,5]);
    host('map',[DATA,1,role==='smc'?7:3]);upload(DATA,data,'literal data page');diagnose(DATA,1024,'initial-data');
    if(role==='normal'){host('map',[4294963200,1,3]);upload(4294963200,data,'literal top page');diagnose(4294963200,1024,'initial-top');}
    if(role==='normal') {
      compile('main',0);
      for(const f of bank.forms.filter(f=>f.axis==='edge'&&(!entries||f.case<2)))for(const flags of[2,0xcd7]){const address=DATA+2*f.id;resetWord(address,f.memory);formSeed(f,flags,flags===2?0:2);call('main',1,entries?'entry form representative':'edge form and FLAGS',1,{form:f.id});watch(address);}
      if(!entries){for(const f of bank.forms.filter(f=>f.axis==='wrap')){formSeed(f,0xcd7);call('main',1,'wrapping effective address',1,{form:f.id});watch(16768);}
        compile('alias',1);for(const f of bank.forms.filter(f=>f.axis==='alias')){formSeed(f,0xcd7);call('alias',1,'legal source address alias',1,{form:f.id});watch(16768);}
        for(const f of bank.forms.filter(f=>f.axis==='alias'&&f.alias==='base'&&f.source===3)){
          seed(f.pc,0xcd7,{3:0xffffffff});call('alias',1,'overflow alias fault',0,{form:f.id});call('alias',1,'overflow alias unchanged retry',0,{form:f.id});
          input(28,Buffer.from(littleWord(17152)).toString('hex'),'explicit overflow address-register different input');call('alias',1,'overflow alias repair completion',1,{form:f.id});watch(17152);
        }
      }
      compile('control',entries?1:2);
      if(!entries){
        const shapes=[{id:'unmapped',address:0x8000,map:0x8000},{id:'none',address:DATA,permission:DATA,bits:0},{id:'write-only',address:DATA,permission:DATA,bits:2},
          {id:'read-only',address:DATA,permission:DATA,bits:1},{id:'cross-unmapped',address:DATA+4095,map:DATA+4096},
          {id:'second-byte-read-only',address:DATA+4095,permission:DATA+4096,bits:1},{id:'overflow',address:0xffffffff}];
        for(const shape of shapes){if(shape.id==='second-byte-read-only'){host('map',[DATA+4096,1,3]);upload(DATA+4096,data,'cross-page data');}
          for(const f of bank.fault_forms.filter(f=>f.kind==='and'||['unmapped','read-only','second-byte-read-only','overflow'].includes(shape.id))){
            if(shape.permission!==undefined)host('protect',[shape.permission,1,shape.bits]);seed(f.prefix,0xcd7,{0:0xb1010000|(f.kind==='and'?0:f.kind==='or'?0xffff:0x8001),3:shape.address});
            call('control',20,`${shape.id} prefix fault`,1,{form:f.id});call('control',1,`${shape.id} unchanged retry`,0,{form:f.id});
            if(shape.id==='read-only'||shape.id==='second-byte-read-only')watch(shape.address);
            if(shape.map!==undefined){host('map',[shape.map,1,3]);upload(shape.map,data,'data-only fault repair');}
            else if(shape.permission!==undefined)host('protect',[shape.permission,1,3]);else input(28,Buffer.from(littleWord(17152)).toString('hex'),'explicit overflow address-register different input');
            call('control',1,`${shape.id} repair completion`,1,{form:f.id});watch(shape.id==='overflow'?17152:shape.address);
            if(shape.map!==undefined)host('unmap',[shape.map,1]);
          }
          if(shape.id==='second-byte-read-only')host('unmap',[DATA+4096,1]);
        }
        for(const address of[DATA+17,DATA+4094,0xfffffffe])for(const f of bank.fault_forms){seed(f.pc,0xcd7,{3:address});call('control',1,'unaligned or last-two-byte store',1,{form:f.id});watch(address);}
        host('map',[DATA+4096,1,3]);upload(DATA+4096,data,'cross-page success data');for(const f of bank.fault_forms){seed(f.pc,0xcd7,{3:DATA+4095});call('control',1,'successful cross-page store',1,{form:f.id});watch(DATA+4095);}diagnose(DATA+4096,1024,'cross-final');host('unmap',[DATA+4096,1]);
      }
      for(const path of bank.consumer.paths){consumerSeed(path);call('control',13,'continuous consumer',12,{path:path.id});watch(17152+2*path.id);
        consumerSeed(path);call('control',3,'split consumer',3,{path:path.id});if(path.id===0){input(96,'01000000','set consumer cancel');call('control',5,'consumer cancel continuation',0,{path:path.id});input(96,'00000000','clear consumer cancel');}
        for(const n of[6,3])call('control',n,'split consumer',n,{path:path.id});call('control',1,'consumer no replay',0,{path:path.id});watch(17152+2*path.id);
      }
      if(!entries){for(const path of bank.consumer.next_paths){nextSeed(path);call('control',7,'current-address continuous',6,{path:path.id});watch(path.id?0x4200:0x4100);watch(path.id?0x4202:0x4102);
        nextSeed(path);for(const n of[1,2,3])call('control',n,'current-address split',n,{path:path.id});call('control',1,'current-address no replay',0,{path:path.id});watch(path.id?0x4200:0x4100);watch(path.id?0x4202:0x4102);}
        const path=bank.consumer.paths[0];seed(path.pc,0xcd7,{3:0x8000});call('control',20,'one-seed repair initial fault',3);call('control',1,'one-seed repair unchanged retry',0);
        host('map',[0x8000,1,3]);upload(0x8000,data,'declared one-seed repair upload');host('protect',[0x8000,1,1]);call('control',1,'one-seed repair read-success write fault',0);watch(0x8000);
        host('protect',[0x8000,1,3]);call('control',1,'one-seed repair store',1);call('control',9,'one-seed repair consumers',8);call('control',1,'one-seed repair no replay',0);diagnose(0x8000,1024,'repair-final');host('unmap',[0x8000,1]);
      }
      const path=bank.consumer.paths[0];seed(path.pc,0xcd7,{3:17152});call('control',0,'zero budget',0);input(96,'01000000','set cancel');call('control',20,'cancel positive budget',0);call('control',0,'cancel zero budget',0);input(96,'00000000','clear cancel');
      const request=Buffer.alloc(entries?8:16);request.writeUInt32LE(path.pc);if(!entries)request.writeUInt32LE(29,4);request.writeUInt32LE(bank.invalid_pc,entries?4:8);if(!entries)request.writeUInt32LE(2,12);
      input(140,request.toString('hex'),'late admitted store then UD2');host(owner==='resident'?entries?'compile_resident_entries':'compile_resident':entries?'compile_entries':'compile',entries?[2,0]:[2],{group:'late-failure',allowed_failure:true});
      seed(path.pc,0xcd7,{3:17152});call('control',1,'prior owner survival',1);add('guard',{group:'control',wrong_key:true});
    }else if(role==='helper'){
      const request=Buffer.alloc(8);request.writeUInt32LE(bank.fault_forms[0].prefix);request.writeUInt32LE(4,4);input(140,request.toString('hex'),'inert helper module descriptors');host('compile',[1],{group:'helper'});
      for(const h of literalHelpers()){add('module',{target:'child',group:'helper',slot:null,file:`${spec.id}-helper-${h.id}.wasm`,helper_case:h.id});seed(bank.fault_forms[0].prefix,0xcd7,{0:0xb1010001,3:DATA});call('helper',20,'malformed inert helper '+h.id,1,{channel:'direct',helper_case:h.id});}
    }else{
      if(owner==='resident'){compile('unrelated',0);compile('other-code',1);compile('changed',2);}else compile('changed',0);
      seed(bank.smc[0].pc,0xcd7,{0:0xb1010001});call('changed',10,'changed own-code store',1);
      call('changed',1,'stale changed direct',0,{channel:'direct'});
      compile('changed-successor',owner==='resident'?3:0);call('changed-successor',3,'fresh successor without replay',2);
      if(owner==='resident'){call('other-code',1,'other unit stale direct',0,{channel:'direct'});seed(DATA+0xe00);call('unrelated',2,'unrelated installed survivor',2);}
      compile('equal',owner==='resident'?4:0);seed(bank.smc[1].pc,0xcd7,{0:0xb1010000});call('equal',10,'same-value own-code store',1);call('equal',0,'same-value stale zero direct',0,{channel:'direct'});
      input(96,'01000000','set stale cancel');call('equal',0,'same-value stale cancel direct',0,{channel:'direct'});input(96,'00000000','clear stale cancel');
      if(owner==='resident'){call('equal',1,'same-value stale dispatcher',0,{channel:'dispatch'});call('equal',0,'stale dispatcher zero budget',0,{channel:'dispatch'});compile('equal-successor',5);call('equal-successor',3,'equal fresh successor without replay',2);}
    }
    diagnose(DATA,1024,'final-data');if(role==='normal')diagnose(4294963200,1024,'final-top');add('close');add('closed_call',{group:role==='normal'?'control':role==='helper'?'helper':owner==='resident'?'equal-successor':'equal',channel:'direct',budget:1,label:'cached closed function',planned_retired:0});
  }
  for(const owner of['replacement','resident'])for(const entries of[false,true])context(owner,entries);
  context('replacement',false,'helper');context('replacement',false,'smc');context('resident',false,'smc');
  return {actions,contexts};
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
  else if(a.name==='write8'){uint(args[0]);uint(args[1],255);c.ram.write(args[0],Buffer.from([args[1]]));record('R3MH',40,[0,0,0,0,0,1],3).copy(c.arena,100);}
  else if(a.name.startsWith('compile')){
    let compiled;
    if(a.allowed_failure){
      assert.equal(a.group,'late-failure');assert.equal(args[0],2);assert.equal(c.arena.readUInt32LE(140),6144);assert.equal(c.arena.readUInt32LE(a.name.endsWith('entries')?144:148),7936);
      if(c.owner==='resident'){assert.ok(c.units.size+1<=8);assert.ok((c.units.size+1)*65536<=524288,'worst-case per-unit cap retains candidate byte reserve before guest');const liveBytes=[...c.units.values()].reduce((n,u)=>n+u.bytes_length,0);assert.ok(liveBytes+65536<=524288,'late decode must retain conservative candidate byte reserve');}
      assert.throws(()=>compileDescriptors(c,a),/excluded UD2 at 1f00/);status=10;
    }
    else compiled=compileDescriptors(c,a);
    if(c.owner==='replacement'){
      if(status){if(physical)assert.deepEqual(e.publication,c.publication);}
      else{const m=physical?e.publication:{generation:c.generation+1,pointer:65536+c.generation*8192,bytes_length:100};assert.equal(m.generation,++c.generation);uint(m.pointer);uint(m.bytes_length,65536);assert.ok(m.pointer>0&&m.bytes_length>8);c.publication=m;c.units.clear();c.live.clear();c.pending={...m,id_low:null,id_high:null};}
    }else if(!status){const words=physical?e.receipt.words:[1,24,c.units.size+1,c.id,65536+c.units.size*8192,100];assert.equal(words.length,6);assert.deepEqual(words.slice(0,2),[1,24]);assert.ok(words[2]||words[3]);assert.ok(words[4]>0&&words[5]>8&&words[5]<=65536);assert.ok(c.units.size<8&&[...c.units.values()].reduce((n,u)=>n+u.bytes_length,0)+words[5]<=524288);assert.ok([...c.units.values()].every(u=>u.id_low!==words[2]||u.id_high!==words[3]));const b=Buffer.alloc(24);words.forEach((n,i)=>b.writeUInt32LE(uint(n),4*i));if(physical)assert.equal(e.receipt.hex,b.toString('hex'));b.copy(c.arena,140);c.pending={generation:null,id_low:words[2],id_high:words[3],pointer:words[4],bytes_length:words[5]};}
    if(!status){assert.deepEqual(compiled.blocks,groups.get(a.group).blocks);assert.equal(compiled.instructions.size,groups.get(a.group).instructions);c.pending={...c.pending,...compiled,group:a.group,installed:false,foreign:null};c.units.set(a.group,c.pending);}
  }else if(a.name==='dispatcher_module'){
    const words=physical?e.receipt.words:[0x50443352,0x10001,32,0,...c.key,131072,100];assert.equal(words.length,8);assert.deepEqual(words.slice(0,6),[0x50443352,0x10001,32,0,...c.key]);assert.ok(words[6]>0&&words[7]>8);const b=Buffer.alloc(32);words.forEach((n,i)=>b.writeUInt32LE(uint(n),i*4));if(physical)assert.equal(e.receipt.hex,b.toString('hex'));b.copy(c.arena,140);c.dispatchMetadata={pointer:words[6],bytes_length:words[7],generation:null,id_low:null,id_high:null};
  }else if(a.name==='acknowledge_resident_installation'){
    const u=c.units.get(a.group);assert.deepEqual(args.slice(0,4),[...c.key,u.id_low,u.id_high]);assert.equal(c.table[args[4]],a.group);u.slot=args[4];u.installed=true;installation(c.arena,u);if(physical){assert.equal(e.receipt.hex,c.arena.subarray(140,172).toString('hex'));assert.deepEqual(e.receipt.words,Array.from({length:8},(_,i)=>c.arena.readUInt32LE(140+i*4)));}
  }else assert.fail('unknown host action');if(physical)assert.equal(e.status,status);return status;
}
export function validatePlan(plan) {
  assert.equal(plan.schema_version,1);assert.equal(plan.size,SIZE);assert.equal(plan.code,CODE);assert.equal(plan.data_address,DATA);assert.equal(plan.top,4294963200);
  assert.deepEqual(plan.bank,literalDescription(),'independent raw bank, instruction metadata and roots');assert.equal(plan.data_hex,literalData().toString('hex'));assert.deepEqual(plan.helper_cases,literalHelpers());
  const schedule=literalSchedule();assert.deepEqual(plan.contexts,schedule.contexts);assert.deepEqual(plan.actions,schedule.actions,'every literal input, seed, data-only repair and publication cohort');
  const count=kind=>schedule.actions.filter(a=>a.kind===kind).length,calls=schedule.actions.filter(a=>['call','closed_call'].includes(a.kind)),data=schedule.actions.filter(a=>a.kind==='data');
  const frames=schedule.actions.reduce((n,a)=>n+(['open','close'].includes(a.kind)?1:a.kind==='closed_call'?0:2),0);
  const expected={contexts:schedule.contexts.length,actions:schedule.actions.length,generated_calls:calls.length,planned_retired:calls.reduce((n,a)=>n+a.planned_retired,0),modules:count('module'),frames,raw_bytes:frames*SIZE,data_files:data.length,diagnostic_reads:data.reduce((n,a)=>n+a.count,0),inputs:count('input'),guards:count('guard'),tables:count('table'),files:10+count('module')+data.length,checkpoints:calls.length+count('guard')+count('close')+data.length+1};
  assert.deepEqual(plan.counts,expected);assert.ok(expected.generated_calls<=600&&expected.modules<=24&&frames<=5000);
  const groups=new Map(literalGroups.map(g=>[g.id,g])),contexts=new Map(),late=[],byContext={},byGroup={},coverage=new Set();let retired=0,positive=0,callsN=0,foreignN=0;
  for(const a of schedule.actions){let c=contexts.get(a.context);
    if(a.kind==='open'){assert.ok(!c);c=fresh(schedule.contexts.find(s=>s.id===a.context));contexts.set(a.context,c);byContext[a.context]={calls:0,positive:0,retired:0};}
    else{
      assert.ok(c);if(a.kind==='closed_call'){assert.ok(c.closed);assert.equal(generated(c,a).status,5);callsN++;byContext[c.id].calls++;continue;}assert.equal(c.closed,false);
      if(a.kind==='input'){const b=Buffer.from(a.hex,'hex');assert.ok(a.offset>=0&&a.offset+b.length<=SIZE);if(b.length===SIZE)assert.deepEqual(b,authoredArena());b.copy(c.arena,a.offset);if(a.offset===0)state(c.arena);}
      else if(a.kind==='host'){if(a.allowed_failure)late.push({context:c.id,units:c.units.size,bytes:[...c.units.values()].reduce((n,u)=>n+u.bytes_length,0)});hostStep(c,a,null,groups,false);}
      else if(a.kind==='module'){if(a.target==='child'){assert.ok(c.units.has(a.group),'replacement current child only');c.units.get(a.group).foreign=a.helper_case===null?null:literalHelpers().find(h=>h.id===a.helper_case);}}
      else if(a.kind==='table'){assert.ok(c.units.has(a.group));c.table[a.slot]=a.group;}
      else if(a.kind==='data'){assert.ok([1,1024].includes(a.count));for(let i=0;i<a.count;i++)read32(c,a.address+4*i);}
      else if(a.kind==='call'){
        const before=state(c.arena),u=c.units.get(a.group);assert.ok(u,'no old replacement child after publication');const op=u.instructions.get(before.pc);
        if(['edge form and FLAGS','entry form representative','legal source address alias','wrapping effective address'].includes(a.label)){
          assert.equal(op.kind,'logical');const key=`${c.id}/${before.pc}/${before.flags}`;assert.ok(!coverage.has(key));coverage.add(key);
          if(a.label==='legal source address alias')assert.ok(op.memory.base===op.source||op.memory.index===op.source);
          if(['legal source address alias','wrapping effective address'].includes(a.label))assert.equal(addressOf(op.memory,before.registers),16768);
        }
        const out=generated(c,a);assert.equal(out.status,/^stale changed|^other unit stale|^same-value stale/.test(a.label)?4:0);assert.equal(out.retired,a.planned_retired,`raw retirement ${c.id}/${a.id}/${a.label}`);
        const row=byContext[c.id];row.calls++;row.positive+=Number(out.retired>0);row.retired+=out.retired;const group=`${c.id}/${a.group}`;byGroup[group]??={calls:0,positive:0,retired:0};byGroup[group].calls++;byGroup[group].positive+=Number(out.retired>0);byGroup[group].retired+=out.retired;
        callsN++;positive+=Number(out.retired>0);retired+=out.retired;if(u.foreign)foreignN+=out.addresses.length;
      }else if(a.kind==='guard')assert.equal(a.wrong_key,true);else if(a.kind==='close')c.closed=true;else assert.fail('unknown literal action');
    }
  }
  assert.equal(callsN,expected.generated_calls);assert.equal(retired,expected.planned_retired);assert.ok([...contexts.values()].every(c=>c.closed));assert.equal(late.length,4);
  for(const row of late.filter(r=>contexts.get(r.context).owner==='resident')){assert.ok(row.units+1<=8);assert.ok(row.bytes+65536<=524288);}
  assert.deepEqual(Object.values(byContext).map(r=>[r.calls,r.positive,r.retired]),[[218,185,250],[28,21,61],[218,185,250],[28,21,61],[3,2,2],[7,3,4],[12,5,8]],'source-derived context ledger reconciled independently before aggregate freeze');
  return {counts:expected,calls:callsN,positive,zero:callsN-positive,retired,foreign_helper_calls:foreignN,by_context:byContext,by_group:byGroup,late_admission:late,engine_or_guest_execution:false};
}
const reviewedPaths = [
  'engine/src/cpu/x86/ir.rs','engine/src/cpu/x86/decode/profile.rs','engine/src/cpu/x86/decode/lower.rs',
  'engine/src/cpu/dbt/region.rs','engine/src/cpu/dbt/wasm/memory.rs','engine/src/cpu/dbt/wasm/integer.rs',
  'engine/tests/cpu_word_and.rs','engine/tests/cpu_word_or_xor.rs','engine/tests/cpu_memory_byte_logical.rs','engine/tests/cpu_word_memory_logical.rs',
  'engine/tests/cpu_word_memory_logical_store.rs','engine/tests/cpu_word_memory_logical_store_legacy.rs',
  'engine/tests/fixtures/p2-word-memory-logical-store/plan.mjs','engine/tests/fixtures/p2-word-memory-logical-store/run.mjs','engine/tests/fixtures/p2-word-memory-logical-store/check.mjs',
  'target/word-memory-logical-store-session/independent-word-memory-logical-store.py','target/word-memory-logical-store-session/supervise.py',
  'target/word-memory-logical-store-session/canonical_guard_v1.py','target/word-memory-logical-store-session/contract-v1.md'
].sort();
function disjointPinMaps(...maps){
  const seen=new Set();
  for(const map of maps){assert.ok(map&&typeof map==='object'&&!Array.isArray(map));for(const path of Object.keys(map)){
    assert.equal(isAbsolute(path),false);assert.ok(path&&!path.split('/').includes('..'));assert.equal(relative('/repository',resolve('/repository',path)),path);
    assert.ok(!seen.has(path),'source, artifact and historical pin domains are disjoint');seen.add(path);
  }}
}
function sourceAuthority(freeze,root){
  assert.equal(freeze.status,'source-reviewed');assert.deepEqual(Object.keys(freeze.engine).sort(),['bytes','sha256']);
  disjointPinMaps(freeze.source_pins,freeze.historical_artifacts,freeze.protected_artifacts);
  for(const name of ['engine_path','plan_path','capture_path','node_path'])assert.ok(isAbsolute(freeze[name]));
  assert.deepEqual(pin(readFileSync(regularFile(freeze.engine_path))),freeze.engine);assert.deepEqual(pin(readFileSync(regularFile(freeze.plan_path))),freeze.plan);
  assert.deepEqual(Object.keys(freeze.node).sort(),['bytes','sha256']);assert.deepEqual(pin(readFileSync(regularFile(freeze.node_path))),freeze.node);
  const rows=Object.keys(freeze.source_pins).sort().map(path=>{assert.deepEqual(pin(readFileSync(safeFile(root,path))),freeze.source_pins[path]);return{path,...freeze.source_pins[path]};});
  for(const path of reviewedPaths)assert.ok(Object.hasOwn(freeze.source_pins,path),'all reviewed owners are in the frozen source authority');
  for(const name of ['historical_artifacts','protected_artifacts']){assert.ok(freeze[name]&&typeof freeze[name]==='object'&&!Array.isArray(freeze[name]));for(const[path,expected]of Object.entries(freeze[name]))assert.deepEqual(pin(readFileSync(safeFile(root,path))),expected,`${name}: ${path}`);}
  assert.equal(freeze.reviews.length,3);assert.equal(new Set(freeze.reviews.map(r=>r.index)).size,3);const expectedReviews=Object.fromEntries(reviewedPaths.map(path=>[path,freeze.source_pins[path]]));
  for(const r of freeze.reviews){assert.equal(r.ack,true);const b=readFileSync(safeFile(root,r.index));assert.deepEqual(pin(b),{bytes:r.bytes,sha256:r.sha256});const review=JSON.parse(b);assert.equal(review.ack,true);assert.deepEqual(review.survivors,[]);assert.deepEqual(review.source_pins,expectedReviews,'three reviews bind the exact current nineteen owners');}
  return rows;
}
function nodeRuntime(freeze,command,environment,root,directory){
  assert.deepEqual(command,[freeze.node_path,join(root,'engine/tests/fixtures/p2-word-memory-logical-store/run.mjs'),freeze.engine_path,directory,root,freeze.capture_path]);
  assert.equal(environment.executable,freeze.node_path);assert.deepEqual(environment.exec_argv,[]);
}
function executionAuthority(root,directory,freezePath,freezeBytes,freeze,result){
  const bankDirectory=dirname(freezePath),bankRelative=relative(root,bankDirectory),supervisorPath=`${bankRelative}/supervise.py`,guardPath=`${bankRelative}/canonical_guard_v1.py`;
  assert.equal(bankRelative,'target/word-memory-logical-store-session');
  const supervisionPath=join(bankDirectory,'freeze-actual-supervision.json');assert.equal(freeze.actual_supervision_freeze_path,supervisionPath);
  const supervisionBytes=readFileSync(regularFile(supervisionPath)),supervision=JSON.parse(supervisionBytes);
  assert.equal(supervision.ack,true);assert.equal(supervision.deadline_seconds,600);assert.deepEqual(supervision.sources,freeze.source_pins);
  disjointPinMaps(supervision.sources,supervision.artifacts);
  assert.deepEqual(supervision.supervisor,freeze.source_pins[supervisorPath]);assert.deepEqual(supervision.supervisor,pin(readFileSync(safeFile(root,supervisorPath))));
  for(const name of ['sources','artifacts']){assert.ok(supervision[name]&&typeof supervision[name]==='object'&&!Array.isArray(supervision[name]));for(const[path,expected]of Object.entries(supervision[name]))assert.deepEqual(pin(readFileSync(safeFile(root,path))),expected,`supervision ${name}: ${path}`);}
  const protectedPin=(path,expected)=>{assert.deepEqual(pin(readFileSync(safeFile(root,path))),expected);assert.ok([freeze.protected_artifacts[path],supervision.artifacts[path]].some(p=>p&&p.bytes===expected.bytes&&p.sha256===expected.sha256),`protected relative evidence: ${path}`);};
  assert.deepEqual(supervision.artifacts[relative(root,freezePath)],pin(freezeBytes));
  for(const map of [freeze.historical_artifacts,freeze.protected_artifacts])for(const[path,expected]of Object.entries(map))assert.deepEqual(supervision.artifacts[path],expected,'supervision protects every historical and frozen artifact');
  for(const path of Object.keys(freeze.source_pins))assert.deepEqual(supervision.sources[path],freeze.source_pins[path]);
  protectedPin(relative(root,freeze.engine_path),freeze.engine);for(const[file,expected]of Object.entries(freeze.capture_pins))protectedPin(relative(root,join(freeze.capture_path,file)),expected);
  const nodeAuthorityPath=`${bankRelative}/node-executable-authority-v1.json`,nodeAuthorityBytes=readFileSync(safeFile(root,nodeAuthorityPath));protectedPin(nodeAuthorityPath,pin(nodeAuthorityBytes));
  assert.deepEqual(JSON.parse(nodeAuthorityBytes),{node_path:freeze.node_path,node:freeze.node});
  const expectedEnvironment={NODE_OPTIONS:'',RING3_ENGINE_SHA256:freeze.engine.sha256,RING3_WORD_MEMORY_LOGICAL_STORE_FREEZE:freezePath};assert.deepEqual(supervision.environment,expectedEnvironment);
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
  const vault='/Users/fatihguzel/Obsidian/personal/fatihguzel/projects/ring3',names=['00_project_index.md','founder_report.md','linear_setup.md','development/2026-10-10.md','research/prototype2/2026-10-10/08_word_memory_logical_store.md'];
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
  for(const row of result.modules){const b=readFileSync(safeFile(directory,row.file));files.add(row.file);assert.deepEqual(pin(b),{bytes:row.bytes_length,sha256:row.sha256});assert.deepEqual(moduleInterface(b,row.owner,row.target==='dispatcher',row.group),{imports:row.imports,exports:row.exports});moduleBinding(b,row,plan.contexts.find(c=>c.id===row.context));}
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
  return{schema_version:1,complete:true,semantic_certification:true,freeze_sha256:hash(freezeBytes),result_sha256:hash(resultBytes),raw_sha256:hash(raw),plan_sha256:hash(planBytes),engine:freeze.engine,execution,counts,positive,zero:counts.generated_calls-positive,independently_replayed_instructions:retired,physical_files:files.size,journal_rows:journal.length,checkpoint_rows:checkpoints.length,observation_limit:'finite authored WORD memory destination AND/OR/XOR, checked read/write FLAGS/EA/RAM/code-currentness/owner proof; full frames only before/after each diagnostic sweep or watched-word read, per-read helper packets visible; no intermediate diagnostic full-arena, arbitrary helper side-effect rollback, exhaustive ISA, platform or performance claim'};
}
export function inspectIncomplete(directory){directory=resolve(directory);const files=readdirSync(directory).sort().map(file=>({file,...pin(readFileSync(safeFile(directory,file)))})),raw=files.find(r=>r.file==='arenas.bin');return{schema_version:1,complete:false,semantic_certification:false,files,available_full_frames:Math.floor((raw?.bytes??0)/SIZE),trailing_frame_bytes:(raw?.bytes??0)%SIZE,observation_limit:'uncertified physical inventory; no partial semantic replay'};}


function selfTest() {
  let assertions=0;const same=(a,b)=>{assert.deepEqual(a,b);assertions++;},reject=fn=>{assert.throws(fn);assertions++;};
  disjointPinMaps({'engine/source.rs':{}},{'target/proof.json':{}},{'target/history.json':{}});assertions++;
  reject(()=>disjointPinMaps({'engine/source.rs':{}},{'engine/source.rs':{}}));reject(()=>disjointPinMaps({'/absolute/live/note':{}}));reject(()=>disjointPinMaps({'target/../escape':{}}));
  {const freeze={node_path:'/pinned/node',engine_path:'/root/engine.wasm',capture_path:'/root/capture'},root='/root',directory='/root/output',command=[freeze.node_path,root+'/engine/tests/fixtures/p2-word-memory-logical-store/run.mjs',freeze.engine_path,directory,root,freeze.capture_path],env={executable:freeze.node_path,exec_argv:[]};nodeRuntime(freeze,command,env,root,directory);assertions++;reject(()=>nodeRuntime(freeze,['node',...command.slice(1)],env,root,directory));reject(()=>nodeRuntime(freeze,command,{...env,executable:'/other/node'},root,directory));reject(()=>nodeRuntime(freeze,command,{...env,exec_argv:['--inspect']},root,directory));}
  const masks=[0xffff0000,0xffff,0xaaaaaaaa,0x55555555,0xf0f0f0f0,0x0f0f0f0f,0xfafafafa,0x05050505,0xff00ff00,0x00ff00ff,0xcccccccc,0x33333333,0x00ffff00,0xff0000ff,0xf0fffff0,0x0f00000f];
  for(let cc=0;cc<16;cc++)for(let n=0;n<32;n++){const flags=2+[1,4,64,128,2048].reduce((v,b,i)=>v+(n>>i&1)*b,0);same(Number(predicate(cc,flags)),Math.floor(masks[cc]/2**n)%2);same(predicate(cc,flags),predicate(cc,flags|1040));}
  for(const[k,l,r,result,flags]of[['and',0,65535,0,0x46],['and',65535,0,0,0x46],['and',32768,65535,32768,0x86],['and',4660,3855,516,2],['or',0,0,0,0x46],['or',0,1,1,2],['or',32767,32768,65535,0x86],['or',254,1,255,6],['xor',65535,65535,0,0x46],['xor',32768,0,32768,0x86],['xor',255,1,254,2],['xor',43690,21845,65535,0x86]]){same(wordLogical(k,l,r,2),{result,flags});same(wordLogical(k,l,r,0xcd7),{result,flags:flags|1024});same(wordLogical(k,l,r,0xcd6),wordLogical(k,l,r,0xcd7));}
  const setup=(writable=true)=>{const c=fresh({id:1,owner:'replacement',entries:false,key:[17,19]});c.arena=authoredArena();c.ram.map(CODE,1,7);c.ram.write(CODE,literalBank());c.ram.map(DATA,1,3);c.ram.write(DATA,literalData());if(!writable)c.ram.protect(DATA,1,1);return c;};
  const install=(c,blocks,group='one',extra={})=>{const b=Buffer.alloc(blocks.length*8);blocks.forEach(([pc,n],i)=>{b.writeUInt32LE(pc,8*i);b.writeUInt32LE(n,8*i+4);});b.copy(c.arena,140);const unit={...compileDescriptors(c,{name:'compile',args:[blocks.length]}),...extra};c.units.set(group,unit);return unit;};
  const ramBytes=c=>[...c.ram.pages].map(([pc,p])=>[pc,p.bits,p.token,p.bytes.toString('hex')]);
  for(const[hex,right,result,flags]of[['662103',0x1234,0x1234,0x402],['660903',0,65535,0x486],['663103',65535,0,0x446]]){
    const c=setup();c.ram.write(DATA,Buffer.from([255,255]));const op=decodeAt(Buffer.from(hex,'hex'),0,CODE),cpu=state(c.arena);cpu.flags=0xcd7;cpu.registers[0]=0xa1010000+right;cpu.registers[3]=DATA;const before=structuredClone(cpu),token=c.ram.pages.get(DATA).token,neighbors=c.ram.read(DATA+2,4).bytes;
    const out=execute(c,op,cpu,null,[]);same(out.cpu,{...before,pc:CODE+3,flags});same(cpu,before);same(c.ram.read(DATA,2).bytes.readUInt16LE(),result);same(c.ram.read(DATA+2,4).bytes,neighbors);same(c.ram.pages.get(DATA).token>token,true);same(c.arena.subarray(100,140),record('R3MH',40,[0,0,0,0,0,2],4));
  }
  for(const[hex,right]of[['662103',0],['660903',65535],['663103',0]])for(const bits of[0,2,4,1]){
    const c=setup(),op=decodeAt(Buffer.from(hex,'hex'),0,CODE),cpu=state(c.arena);cpu.registers[0]=0xb1010000+right;cpu.registers[3]=DATA;c.ram.protect(DATA,1,bits);const before=structuredClone(cpu),ram=ramBytes(c);
    same(execute(c,op,cpu,null,[]),{fault:{reason:5,detail:2,address:DATA,access:bits===1?2:1,length:2}});same(cpu,before);same(ramBytes(c),ram);
  }
  {const c=setup(),cpu=state(c.arena),op=decodeAt(Buffer.from('663103','hex'),0,CODE);cpu.registers[3]=DATA+4095;cpu.registers[0]=0xa101ffff;const before=structuredClone(cpu);same(execute(c,op,cpu,null,[]),{fault:{reason:5,detail:1,address:DATA+4096,access:1,length:2}});
    c.ram.map(DATA+4096,1,3);c.ram.write(DATA+4096,literalData());c.ram.protect(DATA+4096,1,1);const ram=ramBytes(c);same(execute(c,op,cpu,null,[]),{fault:{reason:5,detail:2,address:DATA+4096,access:2,length:2}});same(cpu,before);same(ramBytes(c),ram);c.ram.protect(DATA+4096,1,3);same(execute(c,op,cpu,null,[]).cpu.pc,CODE+3);same(c.ram.read(DATA+4095,2).bytes,Buffer.from([61,255]));}
  {const c=setup(),cpu=state(c.arena),op=decodeAt(Buffer.from('662103','hex'),0,CODE);cpu.registers[3]=U32-1;same(execute(c,op,cpu,null,[]),{fault:{reason:5,detail:3,address:U32-1,access:1,length:2}});c.ram.map(U32-4096,1,3);c.ram.write(U32-4096,literalData());cpu.registers[3]=U32-2;same(execute(c,op,cpu,null,[]).cpu.pc,CODE+3);}
  for(const h of literalHelpers()){const c=setup(),cpu=state(c.arena),op=decodeAt(Buffer.from('662103','hex'),0,CODE);cpu.registers[3]=DATA;const before=structuredClone(cpu),ram=ramBytes(c),addresses=[];same(execute(c,op,cpu,h,addresses),{fault:{reason:7,detail:2}});same(cpu,before);same(ramBytes(c),ram);same(addresses,[DATA]);same(c.arena.subarray(100,140).toString('hex'),h.packet_hex);}
  for(let path=0;path<2;path++){
    const c=setup(),pc=6144+path*64;install(c,[[pc,29],[pc+31,9]]);Buffer.from(literalSeed(pc,0xcd7,{3:17152+2*path,7:0xa808ffff}),'hex').copy(c.arena);const before=Buffer.from(c.arena),ram=ramBytes(c);same(generated(c,{channel:'direct',group:'one',budget:13}).retired,12);
    const want=Buffer.from(before);record('R3ST',56,[path?0xa10100ff:0xa1010100,path?0xa2027fff:0xa2020000,0xa3037f00,0,0xa5058000,0xa6061234,0xa707aaaa,0xa808fffe,pc+40,0x482]).copy(want);record('R3EX',40,[3,12,0,0,0,0],2).copy(want,56);record('R3MH',40,[0,path?32767:0,0,0,0,2],2).copy(want,100);same(c.arena,want);same(c.ram.read(17152+2*path,2).bytes.readUInt16LE(),path?32767:0);
    const d=setup();install(d,[[pc,29],[pc+31,9]]);before.copy(d.arena);for(const n of[3,6,3])same(generated(d,{channel:'direct',group:'one',budget:n}).retired,n);same(state(d.arena),state(want));same(d.ram.read(17152+2*path,2).bytes,c.ram.read(17152+2*path,2).bytes);same(d.arena.subarray(56,96),record('R3EX',40,[1,3,0,0,0,0],2));same(generated(d,{channel:'direct',group:'one',budget:1}).retired,0);
    for(const[label,mutate]of[['source parent',b=>b[18]^=1],['logical undefined flags',b=>b.writeUInt32LE(b.readUInt32LE(52)|0x811,52)],['wrong width',b=>b.writeUInt32LE(4,136)],['wrong retirement',b=>b.writeUInt32LE(11,76)],['wrong PC',b=>b.writeUInt32LE(pc+29,48)],['opaque FP',b=>b[4363]^=1]]){const bad=Buffer.from(want);mutate(bad);reject(()=>assert.deepEqual(bad,want,label));}
  }
  for(let path=0;path<2;path++){
    const c=setup(),pc=6656+64*path;install(c,[[pc,17]]);Buffer.from(literalSeed(pc,0xcd7,{0:path?0xa101fffd:0xa1010002,2:path?0xa3034200:0xa3030f0f,3:path?0x4202:0x4100,6:path?0xa7070001:0xa70700ff}),'hex').copy(c.arena);const before=Buffer.from(c.arena);same(generated(c,{channel:'direct',group:'one',budget:7}).retired,6);
    const want=Buffer.from(before);want.writeUInt32LE(path?0x4201:0x4101,28);want.writeUInt32LE(pc+17,48);want.writeUInt32LE(path?0x402:0x406,52);record('R3EX',40,[3,6,0,0,0,0],2).copy(want,56);record('R3MH',40,[0,0,0,0,0,2],4).copy(want,100);same(c.arena,want);same(c.ram.read(path?0x4200:0x4100,4).bytes,path?Buffer.from([0,1,0,66]):Buffer.from([2,65,0,0]));
    const d=setup();install(d,[[pc,17]]);before.copy(d.arena);for(const n of[1,2,3])same(generated(d,{channel:'direct',group:'one',budget:n}).retired,n);same(state(d.arena),state(want));same(d.ram.read(path?0x4200:0x4100,4).bytes,c.ram.read(path?0x4200:0x4100,4).bytes);
  }
  for(const[id,pc,right,result,flags]of[['changed',7424,1,0x3167,0x402],['equal',7488,0,0x0966,0x406]]){
    const c=setup();install(c,[[pc,10]],'active');Buffer.from(literalSeed(pc,0xcd7,{0:0xb1010000+right}),'hex').copy(c.arena);const before=Buffer.from(c.arena),out=generated(c,{channel:'direct',group:'active',budget:10});same(out.retired,1);same(out.reason,6);same(c.ram.read(pc,2).bytes.readUInt16LE(),result);same(state(c.arena),{...state(before),pc:pc+7,flags});same(c.arena.subarray(56,96),record('R3EX',40,[6,1,0,0,0,0],2));const stale=Buffer.from(c.arena);same(generated(c,{channel:'direct',group:'active',budget:0}).status,4);same(c.arena,stale);install(c,[[pc+7,3]],'fresh');same(generated(c,{channel:'direct',group:'fresh',budget:3}).retired,2);same(c.arena.subarray(56,96),record('R3EX',40,[3,2,0,0,0,0]));
  }
  {const c=setup();c.owner='resident';const old=install(c,[[7431,3]],'old',{installed:true,id_low:1,id_high:0,slot:0});c.ram.write(7424,Buffer.from([103]));install(c,[[7431,3]],'new',{installed:true,id_low:2,id_high:0,slot:1});c.arena.writeUInt32LE(7431,48);same(generated(c,{channel:'dispatch',group:'old',budget:3}).retired,2);same(c.arena.subarray(140,172),record('R3IN',32,[2,0,1,0]));same(c.arena.subarray(56,96),record('R3EX',40,[3,2,0,0,0,0],3));}
  {const c=setup(false);install(c,[[5632,4]]);Buffer.from(literalSeed(5632,0xcd7,{0:0xb1010000,3:DATA}),'hex').copy(c.arena);const before=Buffer.from(c.arena),ram=ramBytes(c);same(generated(c,{channel:'direct',group:'one',budget:20}).retired,1);
    const want=Buffer.from(before);want.writeUInt32LE(5633,48);record('R3EX',40,[5,1,2,DATA,2,2],2).copy(want,56);record('R3MH',40,[1,0,2,DATA,2,2],4).copy(want,100);same(c.arena,want);same(ramBytes(c),ram);same(generated(c,{channel:'direct',group:'one',budget:1}).retired,0);same(state(c.arena),state(want));
    for(const[label,mutate]of[['FLAGS before write',b=>b.writeUInt32LE(0x446,52)],['fault PC advanced',b=>b.writeUInt32LE(5636,48)],['wrong helper access',b=>b.writeUInt32LE(1,132)]]){const bad=Buffer.from(want);mutate(bad);reject(()=>assert.deepEqual(bad,want,label));}
    const badRAM=structuredClone(ram);badRAM[1][3]='01'+badRAM[1][3].slice(2);reject(()=>assert.deepEqual(badRAM,ram,'partial write before second-byte check'));
    c.ram.protect(DATA,1,3);same(generated(c,{channel:'direct',group:'one',budget:1}).retired,1);same(c.arena.subarray(56,96),record('R3EX',40,[1,1,0,0,0,0],2));same(state(c.arena).flags,0x446);same(c.ram.read(DATA,2).bytes,Buffer.from([0,0]));same(c.arena.subarray(4236),before.subarray(4236));
  }
  for(const entries of[false,true]){const c=setup(),a={name:entries?'compile_entries':'compile',args:[1]};c.arena.writeUInt32LE(6144,140);if(!entries)c.arena.writeUInt32LE(29,144);same(compileDescriptors(c,a).instructions.size,9);a.args[0]=2;c.arena.writeUInt32LE(7936,entries?144:148);if(!entries)c.arena.writeUInt32LE(2,152);reject(()=>compileDescriptors(c,a));}
  for(const channel of['direct','dispatch'])for(const cancel of[0,1]){const c=setup();install(c,[[5633,3]],'one',{installed:true,id_low:3,id_high:7,slot:0});c.arena.writeUInt32LE(5633,48);c.arena.writeUInt32LE(cancel,96);same(generated(c,{channel,group:'one',budget:0}).reason,cancel?2:1);c.ram.write(CODE,Buffer.from([102]));const old=Buffer.from(c.arena);same(generated(c,{channel,group:'one',budget:0}).status,channel==='direct'?4:0);if(channel==='direct')same(c.arena,old);c.closed=true;same(generated(c,{channel,group:'one',budget:1}).status,5);}
  for(const hex of['66','6621','660904','663105000000','66662103','67662103','f2662103','6621c3','6609c3','6683d7'])reject(()=>decodeAt(Buffer.from(hex,'hex'),0,CODE));
  for(const excess of['units','bytes']){const c=setup();c.owner='resident';c.arena.writeUInt32LE(6144,140);c.arena.writeUInt32LE(29,144);c.arena.writeUInt32LE(7936,148);c.arena.writeUInt32LE(2,152);for(let i=0;i<(excess==='units'?8:1);i++)c.units.set('prior'+i,{bytes_length:excess==='bytes'?500000:100});reject(()=>hostStep(c,{name:'compile_resident',args:[2],group:'late-failure',allowed_failure:true},null,new Map(literalGroups.map(g=>[g.id,g])),false));}
  function signed(n){let value=BigInt.asIntN(32,BigInt(n));const b=[];for(;;){let byte=Number(value&127n);value>>=7n;const done=value===0n&&!(byte&64)||value===-1n&&Boolean(byte&64);if(!done)byte|=128;b.push(byte);if(done)return b;}}
  function leb(n){const b=[];do{let x=n%128;n=Math.floor(n/128);if(n)x|=128;b.push(x);}while(n);return b;}
  function boundBody(values,memory=true,dispatch=false){const b=dispatch?[1,5,127]:memory?[3,16,127,1,126,6,127]:[2,16,127,1,126];for(const n of values)b.push(65,...signed(n));b.push(32,0,32,1,32,3,16,0,34,dispatch?5:16,4,64,32,dispatch?5:16,15,11,dispatch?65:63,0,11);const section=[1,...leb(b.length),...b];return Buffer.from([0,97,115,109,1,0,0,0,10,...leb(section.length),...section]);}
  for(const owner of['replacement','resident'])for(const memory of[false,true]){const spec={key:[17,19]},row={owner,target:'child',group:memory?'main':'changed-successor',generation:7,id_low:11,id_high:13},values=owner==='replacement'?[17,19,7]:[17,19,11,13],bytes=boundBody(values,memory);moduleBinding(bytes,row,spec);assertions++;reject(()=>moduleBinding(bytes,row,{key:[18,19]}));reject(()=>moduleBinding(bytes,{...row,generation:8,id_low:12},spec));reject(()=>moduleBinding(bytes,{...row,group:memory?'changed-successor':'main'},spec));}
  moduleBinding(boundBody([17,19],false,true),{owner:'resident',target:'dispatcher',group:'dispatcher'},{key:[17,19]});assertions++;
  return{pure_assertions:assertions,engine_or_guest_execution:false};
}
function mutationTest(plan){const rejected=[];const reject=(name,mutate)=>{const copy=structuredClone(plan);mutate(copy);assert.throws(()=>validatePlan(copy),name);rejected.push(name);};validatePlan(plan);
  const seed=(p,label)=>{const a=p.actions.find(a=>a.kind==='call'&&a.label===label);assert.ok(a);const s=p.actions[a.id-1];assert.equal(s.kind,'input');return s;};
  reject('raw suppressed write/read',p=>{const b=Buffer.from(p.bank.hex,'hex');b[1]=0x23;p.bank.hex=b.toString('hex');});
  reject('memory width changed',p=>{const b=Buffer.from(p.bank.hex,'hex');b[0]=0x90;p.bank.hex=b.toString('hex');});
  reject('source/address parent changed',p=>{const b=Buffer.from(p.bank.hex,'hex');b[2]^=8;p.bank.hex=b.toString('hex');});
  reject('instruction truncation',p=>p.bank.groups[0].blocks[0][1]--);reject('missing full frame',p=>p.counts.frames--);reject('missing owner',p=>p.contexts.pop());reject('wrong key',p=>p.contexts[0].key[0]++);
  reject('inert store failure becomes success',p=>{const b=Buffer.from(p.helper_cases[1].packet_hex,'hex');b.writeUInt32LE(2,36);p.helper_cases[1].packet_hex=b.toString('hex');});
  reject('wrong high source parent',p=>{const s=seed(p,'edge form and FLAGS'),b=Buffer.from(s.hex,'hex');b[18]^=1;s.hex=b.toString('hex');});
  reject('overflow repairs wrong register',p=>p.actions.find(a=>a.kind==='input'&&a.offset===28).offset=24);
  reject('retry replays committed prefix',p=>p.actions.find(a=>a.kind==='call'&&a.label.endsWith('unchanged retry')).planned_retired=1);
  reject('second byte write fault has unreadable byte',p=>{const a=p.actions.find(a=>a.kind==='call'&&a.label==='second-byte-read-only prefix fault'),protect=p.actions.slice(0,a.id).findLast(a=>a.kind==='host'&&a.name==='protect');protect.args[2]=2;});
  reject('data-only repair reseeds CPU',p=>p.actions.find(a=>a.kind==='input'&&a.label==='declared one-seed repair upload').offset=0);
  reject('setter-current-EA replaced by old address',p=>{const b=Buffer.from(p.bank.hex,'hex');b[2560+11]=0xc2;p.bank.hex=b.toString('hex');});
  reject('intermediate RAM secretly reset',p=>{const a=p.actions.find(a=>a.kind==='call'&&a.label==='split consumer');a.kind='host';a.name='write8';a.args=[17152,0];});
  reject('equal real store skipped',p=>{const b=Buffer.from(p.bank.hex,'hex');b[3392]=0x90;p.bank.hex=b.toString('hex');});
  reject('post-write retirement missing',p=>p.actions.find(a=>a.kind==='call'&&a.label==='same-value own-code store').planned_retired=0);
  reject('SMC successor replays old store',p=>p.actions.find(a=>a.kind==='call'&&a.label==='fresh successor without replay').group='changed');
  reject('unrelated membership replaced with stale producer',p=>p.actions.find(a=>a.kind==='call'&&a.label==='unrelated installed survivor').group='other-code');
  reject('late valid descriptor omitted',p=>{const a=p.actions.find(a=>a.kind==='input'&&a.label==='late admitted store then UD2'),b=Buffer.from(a.hex,'hex');b.writeUInt32LE(7936);a.hex=b.toString('hex');});
  reject('diagnostic width guessed',p=>p.actions.find(a=>a.kind==='data'&&a.count===1).count=2);reject('closed pointer read',p=>p.actions.find(a=>a.kind==='closed_call').kind='call');
  reject('opaque FP overwritten',p=>p.actions.find(a=>a.kind==='input'&&a.offset===96).offset=4236);
  return{mutations:rejected.length,rejected,engine_or_guest_execution:false};
}
if(process.argv[1]&&resolve(process.argv[1])===fileURLToPath(import.meta.url)) {
  if(process.argv[2]==='--self-test')console.log(JSON.stringify(selfTest(),null,2));
  else if(process.argv[2]==='--mutations'){assert.equal(process.argv.length,4);console.log(JSON.stringify(mutationTest(JSON.parse(readFileSync(process.argv[3]))),null,2));}
  else if(process.argv[2]==='--plan'){assert.equal(process.argv.length,4);console.log(JSON.stringify(validatePlan(JSON.parse(readFileSync(process.argv[3]))),null,2));}
  else{assert.equal(process.argv.length,4,'usage: check.mjs OUTPUT FREEZE');const r=JSON.parse(readFileSync(join(resolve(process.argv[2]),'result.json')));const receipt=r.status==='complete'?checkCapsule(process.argv[2],process.argv[3]):inspectIncomplete(process.argv[2]);console.log(JSON.stringify(receipt,null,2));if(!receipt.complete)process.exitCode=2;}
}
