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
  uint(offset, bytes.length - 1); uint(pc); let at = offset;
  const byte = () => {assert.ok(at < bytes.length, 'truncated instruction'); return bytes[at++];};
  const displacement = n => {let value = 0; for (let i = 0; i < n; i++) value += byte() * 256 ** i; return n === 1 && value >= 128 ? value - 256 : value;};
  const finish = op => ({...op, pc, length: at - offset, next: (pc + at - offset) % U32});
  let opcode = byte(), word = false; if (opcode === 102) {word = true; opcode = byte();}
  function operand(modrm) {
    const mode = modrm >> 6, rm = modrm & 7;
    if (mode === 3) return {register: rm};
    let base = rm, index = null, scale = 1, disp = 0;
    if (rm === 4) {const sib = byte(); base = sib & 7; index = sib >> 3 & 7; scale = 2 ** (sib >> 6); if (index === 4) index = null;}
    if (mode === 0 && base === 5) {base = null; disp = displacement(4);}
    if (mode === 1) disp += displacement(1); else if (mode === 2) disp += displacement(4);
    return {base, index, scale, displacement: ((disp % U32) + U32) % U32};
  }
  if (opcode === 15) {
    const sub = byte(); if (!word && sub === 11) assert.fail(`excluded UD2 at ${pc.toString(16)}`);
    const modrm = byte();
    if (sub >= 64 && sub <= 79) return finish({kind: 'cmov', width: word ? 2 : 4, cc: sub - 64, destination: modrm >> 3 & 7, source: operand(modrm)});
    assert.ok(!word && sub >= 144 && sub <= 159 && modrm >= 192 && modrm < 200, 'authored SETcc only');
    return finish({kind: 'set', cc: sub - 144, alias: modrm & 7});
  }
  if (word && opcode === 57) {const modrm = byte(); assert.ok(modrm >= 192); return finish({kind: 'compare', left: modrm & 7, right: modrm >> 3 & 7});}
  assert.equal(word, false, 'unrecognized WORD prefix or opcode');
  if (opcode === 144) return finish({kind: 'nop'});
  if (opcode === 235 || opcode >= 112 && opcode <= 127) {const disp = displacement(1); return finish({kind: 'branch', cc: opcode === 235 ? null : opcode - 112, target: ((pc + 2 + disp) % U32 + U32) % U32});}
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
function execute(c, op, cpu, foreign, addresses) {
  const old = cpu.registers, registers = [...old]; let flags = cpu.flags, pc = op.next;
  if (op.kind === 'cmov') {
    let value;
    if (op.source.register === undefined) {
      assert.equal(op.width, 2, 'authored memory WORD only'); const address = addressOf(op.source, old); addresses.push(address);
      const checked = validateNarrow(c.arena, address, narrowPacket(c, address, foreign)); if (checked.reason) return {fault: checked}; value = checked.value;
    } else value = old[op.source.register];
    if (predicate(op.cc, flags)) registers[op.destination] = op.width === 2 ? old[op.destination] - old[op.destination] % 65536 + value % 65536 : value;
  } else if (op.kind === 'compare') {
    const left = old[op.left] % 65536, right = old[op.right] % 65536, value = (left - right + 65536) % 65536;
    const parity = Array.from({length: 8}, (_, i) => Math.floor(value / 2 ** i) % 2).reduce((a, b) => a + b, 0) % 2 === 0;
    const signLeft = left >= 32768, signRight = right >= 32768, signValue = value >= 32768;
    flags = ((flags & ~0x8d5) | Number(left < right) | Number(parity) * 4 | Number(left % 16 < right % 16) * 16 | Number(value === 0) * 64 | Number(signValue) * 128 | Number(signLeft !== signRight && signValue !== signLeft) * 2048) >>> 0;
  } else if (op.kind === 'set') {const parent = op.alias % 4, place = op.alias < 4 ? 1 : 256; registers[parent] += (Number(predicate(op.cc, flags)) - Math.floor(old[parent] / place) % 256) * place;}
  else if (op.kind === 'branch') {if (op.cc === null || predicate(op.cc, flags)) pc = op.target;}
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


function literalBank() {
  const b=Buffer.alloc(4096,204); let at=0; const put=(cc,d,tail)=>{const raw=Buffer.from([102,15,64+cc,...tail.map((v,i)=>i? v:v|(d<<3))]);raw.copy(b,at);at+=raw.length;};
  const le=n=>[n%256,Math.floor(n/256)%256,Math.floor(n/65536)%256,Math.floor(n/16777216)%256];
  for(let cc=0;cc<16;cc++)put(cc,cc%8,[5,...le(DATA+cc*2)]);b.set([235,254],at);
  at=256;for(let d=0;d<8;d++){put(4,d,d===4?[4,36]:d===5?[69,0]:[d]);if(d!==4){put(4,d,[4,d*8+5,...le(DATA)]);put(4,d,d===5?[68,d*9,0]:[4,d*9]);}}b.set([235,254],at);
  at=512;for(const[d,tail]of[[3,[131,0,80,0,0]],[6,[4,245,0,64,0,0]],[1,[132,11,0,64,0,0]],[0,[132,48,0,64,0,0]],[5,[132,61,0,64,0,0]],[2,[4,149,0,64,0,0]],[3,[67,255]],[4,[68,36,255]]])put(4,d,tail);b.set([235,254],at);
  at=768;for(let cc=0;cc<16;cc++){b[at++]=144;put(cc,cc%8,[3]);}b.set([235,254],at);
  for(const[i,hex]of['6639c8660f4c13660f4d6b020f4cf50f90c40f92c17c02','6639c8660f4213660f436b020f42f50f92c40f90c17202'].entries()){const o=2048+i*64;Buffer.from(hex,'hex').copy(b,o);b.set([15,11,235,0],o+23);}b.set([15,11],3840);return b;
}
function literalHelpers(){const rows=[],good=record('R3MH',40,[0,32769,0,0,0,2],2);for(const[id,status,at,value]of[['status',9,null,0],['version',0,4,65537],['width',0,36,4],['value',0,20,65536],['span',0,null,0],['overflow',0,null,0]]){const p=Buffer.from(good);if(at!==null)p.writeUInt32LE(value,at);if(id==='span'||id==='overflow'){p.writeUInt32LE(1,16);p.writeUInt32LE(0,20);p.writeUInt32LE(id==='span'?1:3,24);p.writeUInt32LE(id==='span'?DATA+2:DATA,28);p.writeUInt32LE(1,32);}rows.push({id,status,packet_hex:p.toString('hex')});}return rows;}
function literalData(){const b=Buffer.from(Array.from({length:4096},(_,i)=>(73*i+11)%256));b.set([1,128,52,18]);return b;}
const literalGroups=[{id:'main',blocks:[[4096,130],[4352,135],[4608,66]],instructions:49},{id:'control',blocks:[[4864,82],[6144,23],[6169,2],[6208,23],[6233,2]],instructions:49},{id:'helper',blocks:[[4884,5]],instructions:2},{id:'keeper',blocks:[[6147,4]],instructions:1}];
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
  else if(a.name==='write8'){assert.deepEqual(args,[CODE,102]);c.ram.write(args[0],Buffer.from([args[1]]));record('R3MH',40,[0,0,0,0,0,1],3).copy(c.arena,100);}
  else if(a.name.startsWith('compile')){
    let compiled;
    if(a.allowed_failure){assert.equal(a.group,'late-failure');assert.equal(args[0],2);assert.equal(c.arena.readUInt32LE(140),6147);assert.equal(c.arena.readUInt32LE(a.name.endsWith('entries')?144:148),7936);assert.throws(()=>compileDescriptors(c,a),/excluded UD2 at 1f00/);status=10;}
    else compiled=compileDescriptors(c,a);
    if(c.owner==='replacement'){
      if(status){if(physical)assert.deepEqual(e.publication,c.publication);}
      else{const m=physical?e.publication:{generation:c.generation+1,pointer:65536+c.generation*8192,bytes_length:100};assert.equal(m.generation,++c.generation);uint(m.pointer);uint(m.bytes_length);assert.ok(m.pointer>0&&m.bytes_length>8);c.publication=m;c.units.clear();c.live.clear();c.pending={...m,id_low:null,id_high:null};}
    }else if(!status){const words=physical?e.receipt.words:[1,24,c.units.size+1,c.id,65536+c.units.size*8192,100];assert.equal(words.length,6);assert.deepEqual(words.slice(0,2),[1,24]);assert.ok(words[2]||words[3]);assert.ok(words[4]>0&&words[5]>8);assert.ok([...c.units.values()].every(u=>u.id_low!==words[2]||u.id_high!==words[3]));const b=Buffer.alloc(24);words.forEach((n,i)=>b.writeUInt32LE(uint(n),4*i));if(physical)assert.equal(e.receipt.hex,b.toString('hex'));b.copy(c.arena,140);c.pending={generation:null,id_low:words[2],id_high:words[3],pointer:words[4],bytes_length:words[5]};}
    if(!status){assert.deepEqual(compiled.blocks,groups.get(a.group).blocks);c.pending={...c.pending,...compiled,group:a.group,installed:false,foreign:null};c.units.set(a.group,c.pending);}
  }else if(a.name==='dispatcher_module'){
    const words=physical?e.receipt.words:[0x50443352,0x10001,32,0,...c.key,131072,100];assert.equal(words.length,8);assert.deepEqual(words.slice(0,6),[0x50443352,0x10001,32,0,...c.key]);assert.ok(words[6]>0&&words[7]>8);const b=Buffer.alloc(32);words.forEach((n,i)=>b.writeUInt32LE(uint(n),i*4));if(physical)assert.equal(e.receipt.hex,b.toString('hex'));b.copy(c.arena,140);c.dispatchMetadata={pointer:words[6],bytes_length:words[7],generation:null,id_low:null,id_high:null};
  }else if(a.name==='acknowledge_resident_installation'){
    const u=c.units.get(a.group);assert.deepEqual(args.slice(0,4),[...c.key,u.id_low,u.id_high]);assert.equal(c.table[args[4]],a.group);u.slot=args[4];u.installed=true;installation(c.arena,u);if(physical){assert.equal(e.receipt.hex,c.arena.subarray(140,172).toString('hex'));assert.deepEqual(e.receipt.words,Array.from({length:8},(_,i)=>c.arena.readUInt32LE(140+i*4)));}
  }else assert.fail('unknown host action');if(physical)assert.equal(e.status,status);return status;
}
export function validatePlan(plan){
  assert.equal(plan.schema_version,1);assert.equal(plan.size,SIZE);assert.equal(plan.code,CODE);assert.equal(plan.data_address,DATA);assert.equal(plan.top,0xfffff000);
  assert.equal(plan.bank.hex,literalBank().toString('hex'));assert.equal(plan.data_hex,literalData().toString('hex'));assert.deepEqual(plan.bank.groups,literalGroups);assert.equal(plan.contexts.length,7);assert.equal(new Set(plan.contexts.map(x=>x.id)).size,7);
  assert.deepEqual(plan.contexts.map(c=>[c.owner,c.entries,c.role]),[['replacement',false,'normal'],['replacement',true,'normal'],['resident',false,'normal'],['resident',true,'normal'],['replacement',false,'helper'],['replacement',false,'stale'],['resident',false,'stale']]);
  for(const c of plan.contexts){assert.equal(c.pages,6);assert.deepEqual(c.key,[0x57430000+c.id,0x4d434d56]);}
  assert.deepEqual(plan.actions.map(a=>a.id),plan.actions.map((_,i)=>i));
  const expected={contexts:7,actions:8811,generated_calls:4236,planned_retired:4064,modules:19,frames:17594,raw_bytes:76780216,data_files:24,diagnostic_reads:24576,inputs:3874,guards:4,tables:5,files:53,checkpoints:4272};assert.deepEqual(plan.counts,expected);
  assert.equal(plan.helper_cases.length,6);assert.deepEqual(plan.helper_cases.map(h=>h.id),['status','version','width','value','span','overflow']);
  assert.deepEqual(plan.helper_cases,literalHelpers());
  const inputs=plan.actions.filter(a=>a.kind==='input');assert.equal(inputs.filter(a=>a.offset===0&&a.hex.length===SIZE*2).length,7);for(const a of inputs){uint(a.offset,SIZE);assert.match(a.hex,/^(?:[0-9a-f]{2})+$/);assert.ok(a.offset+a.hex.length/2<=SIZE);}
  const calls=plan.actions.filter(a=>a.kind==='call');assert.equal(calls.filter(a=>a.label==='predicate truth').length,3072);assert.equal(calls.filter(a=>a.label==='entry predicate representative').length,64);assert.equal(calls.filter(a=>a.label==='base').length,32);assert.equal(calls.filter(a=>a.label==='index').length,28);assert.equal(calls.filter(a=>a.label==='both').length,28);assert.equal(calls.filter(a=>a.label==='wrap').length,32);
  assert.equal(calls.filter(a=>a.label.endsWith('prefix fault')).length,238);assert.equal(calls.filter(a=>a.label.endsWith('unchanged retry')).length,238);assert.equal(calls.filter(a=>a.label.endsWith('repair completion')).length,238);
  assert.equal(calls.filter(a=>a.label==='last-two-byte read').length,128);assert.equal(calls.filter(a=>a.label==='successful cross-page read').length,64);assert.equal(calls.filter(a=>a.label==='continuous consumer').length,8);assert.equal(calls.filter(a=>a.label==='split consumer').length,24);
  const groups=new Map(literalGroups.map(g=>[g.id,g])),contexts=new Map(),truth=new Map();let retired=0,positive=0,callsN=0,foreignN=0;
  for(const a of plan.actions){let c=contexts.get(a.context);if(a.kind==='open'){assert.ok(!c);c=fresh(plan.contexts.find(s=>s.id===a.context));contexts.set(a.context,c);}
    else{assert.ok(c);if(a.kind==='closed_call'){assert.ok(c.closed);const out=generated(c,a);assert.equal(out.status,5);callsN++;continue;}assert.equal(c.closed,false);
      if(a.kind==='input'){const b=Buffer.from(a.hex,'hex');if(b.length===SIZE)assert.deepEqual(b,authoredArena());b.copy(c.arena,a.offset);if(a.offset===0)state(c.arena);}
      else if(a.kind==='host')hostStep(c,a,null,groups,false);
      else if(a.kind==='module'){if(a.target==='child')c.units.get(a.group).foreign=a.helper_case===null?null:plan.helper_cases.find(h=>h.id===a.helper_case);}
      else if(a.kind==='table')c.table[a.slot]=a.group;
      else if(a.kind==='data'){assert.equal(a.count,1024);for(let i=0;i<a.count;i++)read32(c,a.address+4*i);}
      else if(a.kind==='call'){
        const before=state(c.arena),op=c.units.get(a.group).instructions.get(before.pc);
        if(a.label==='predicate truth'){
          assert.equal(op.kind,'cmov');assert.equal(op.width,2);assert.equal(op.destination,op.cc%8);assert.deepEqual(op.source,{base:null,index:null,scale:1,displacement:DATA+op.cc*2});
          const pattern=(Math.floor(before.registers[0]/65536)-0xa101)/0x111;uint(pattern,2);const lows=[[0,1,32767,65535,32768,4660,43690,21845],[32767,65535,0,32768,1,43690,21845,4660],[32768,32767,65535,32767,4660,1,21845,43690]][pattern];
          assert.deepEqual(before.registers,lows.map((n,i)=>(0xa101+pattern*0x111+i*0x101)*65536+n));assert.equal(before.flags&0x410,[0,16,1024][pattern]);
          const key=`${op.cc}/${before.flags}/${pattern}`;const seen=truth.get(c.id)??new Set();assert.ok(!seen.has(key),'unique truth input');seen.add(key);truth.set(c.id,seen);
        }
        if(['base','index','both','wrap'].includes(a.label)){assert.equal(op.kind,'cmov');assert.equal(op.cc,4);assert.equal(addressOf(op.source,before.registers),DATA);assert.ok([0x412,0x452].includes(before.flags));}
        if(a.label.endsWith('prefix fault')){assert.equal(op.kind,'nop');const cmov=c.units.get(a.group).instructions.get(op.next);assert.equal(predicate(cmov.cc,before.flags),a.label.includes(' true '));}
        const out=generated(c,a);assert.equal(out.retired,a.planned_retired,`prospective retirement ${a.id} ${a.label}`);retired+=out.retired;positive+=Number(out.retired>0);callsN++;if(c.units.get(a.group)?.foreign)foreignN+=out.addresses.length;}
      else if(a.kind==='guard')assert.equal(a.wrong_key,true);
      else if(a.kind==='close')c.closed=true;else assert.fail('unknown plan action');}}
  assert.deepEqual([...truth].map(([id,seen])=>[id,seen.size]),[[1,1536],[3,1536]]);assert.equal(retired,4064);assert.equal(callsN,4236);assert.equal(foreignN,6);assert.ok([...contexts.values()].every(c=>c.closed));return{calls:callsN,positive,zero:callsN-positive,retired,foreign_calls:foreignN};
}

function sourceAuthority(freeze,root){
  assert.equal(freeze.status,'source-reviewed');assert.deepEqual(Object.keys(freeze.engine).sort(),['bytes','sha256']);
  for(const name of ['engine_path','plan_path','capture_path'])assert.ok(isAbsolute(freeze[name]));
  assert.deepEqual(pin(readFileSync(regularFile(freeze.engine_path))),freeze.engine);assert.deepEqual(pin(readFileSync(regularFile(freeze.plan_path))),freeze.plan);
  const rows=Object.keys(freeze.source_pins).sort().map(path=>{assert.deepEqual(pin(readFileSync(safeFile(root,path))),freeze.source_pins[path]);return{path,...freeze.source_pins[path]};});
  for(const name of ['plan','run','check'])assert.ok(rows.some(r=>r.path===`engine/tests/fixtures/p2-word-memory-conditional-move/${name}.mjs`));
  assert.ok(rows.some(r=>r.path==='engine/tests/cpu_word_memory_conditional_move_wasm.rs'));
  assert.equal(freeze.reviews.length,3);for(const r of freeze.reviews){assert.equal(r.ack,true);assert.deepEqual(pin(readFileSync(safeFile(root,r.index))),{bytes:r.bytes,sha256:r.sha256});}return rows;
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
  const modules=new Map(result.modules.map(r=>[r.file,r])),dataFiles=new Map(result.data_files.map(r=>[r.file,r]));assert.equal(modules.size,19);assert.equal(dataFiles.size,24);
  const files=new Set([...captures,'plan.json','manifest.json','result.json','arenas.bin','journal.ndjson','checkpoints.ndjson']);
  for(const row of result.modules){const b=readFileSync(safeFile(directory,row.file));files.add(row.file);assert.deepEqual(pin(b),{bytes:row.bytes_length,sha256:row.sha256});assert.deepEqual(moduleInterface(b,row.owner,row.target==='dispatcher'),{imports:row.imports,exports:row.exports});moduleBinding(b,row,plan.contexts.find(c=>c.id===row.context));}
  for(const row of result.data_files){files.add(row.file);assert.deepEqual(pin(readFileSync(safeFile(directory,row.file))),{bytes:row.bytes_length,sha256:row.sha256});assert.equal(row.bytes_length,4096);}
  assert.equal(files.size,53);assert.deepEqual(readdirSync(directory).sort(),[...files].sort());assert.equal(result.contexts.length,7);assert.equal(new Set(result.contexts.map(c=>c.id)).size,7);
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
        const row=dataFiles.get(a.file);assert.ok(row&&!seenData.has(row.file));seenData.add(row.file);assert.deepEqual(row,{context:c.id,event:a.id,file:a.file,address:a.address,count:1024,bytes_length:4096,sha256:row.sha256});assert.equal(e.data_file,row.file);assert.equal(e.data_sha256,row.sha256);const b=Buffer.alloc(4096),reads=[];
        for(let i=0;i<1024;i++){const r=read32(c,a.address+4*i);reads.push(r);b.writeUInt32LE(r.value,i*4);}assert.deepEqual(e.reads,reads,'every diagnostic status/value/helper packet');assert.deepEqual(readFileSync(safeFile(directory,row.file)),b);counts.host_api_calls+=1024;counts.diagnostic_reads+=1024;counts.data_files++;wantedJournal.push({kind:'data_file',...row});
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
  assert.deepEqual(journal,wantedJournal);assert.equal(frameIndex,result.frames.length);assert.equal(seenModules.size,19);assert.equal(seenData.size,24);assert.ok([...contexts.values()].every(c=>c.closed));assert.deepEqual(result.observed_counts,counts);assert.deepEqual(result.observed_statuses,statuses);assert.equal(positive,prediction.positive);assert.equal(retired,prediction.retired);assert.equal(raw.length,76780216);assert.equal(journal.length,26448);
  const checkpointEvents=plan.actions.filter(a=>['call','closed_call','guard','close','data'].includes(a.kind)).map(a=>a.id).concat(plan.actions.at(-1).id);assert.deepEqual(checkpoints.map(r=>r.last_event),checkpointEvents);assert.equal(checkpoints.length,4272);
  const offsets=new Map();let end=0;for(const line of journalBytes.toString('utf8').slice(0,-1).split('\n')){end+=Buffer.byteLength(line)+1;const r=JSON.parse(line);if(r.kind==='event')offsets.set(r.event.action_id,end);}for(const[i,r]of checkpoints.entries()){const s=snapshots.get(r.last_event);assert.deepEqual(r,{index:i,events:s.events,frames:s.frames,raw_bytes:s.frames*SIZE,journal_bytes:offsets.get(r.last_event),last_event:r.last_event,observed_counts:s});}
  const bankDirectory=dirname(freezePath),started=JSON.parse(readFileSync(safeFile(bankDirectory,'actual-supervisor-started.json'))),supervised=JSON.parse(readFileSync(safeFile(bankDirectory,'actual-supervisor.json')));
  assert.equal(started.freeze_sha256,hash(freezeBytes));assert.deepEqual(started.command,['node',join(root,'engine/tests/fixtures/p2-word-memory-conditional-move/run.mjs'),freeze.engine_path,directory,root,freeze.capture_path]);assert.equal(started.deadline_seconds,600);assert.equal(started.cleanup_reserve_seconds,5);assert.equal(started.sources,sources.length);assert.equal(started.NODE_OPTIONS,'');assert.deepEqual(started.engine,freeze.engine);for(const[k,v]of Object.entries(started))assert.deepEqual(supervised[k],v);
  assert.equal(supervised.exit_code,0);assert.equal(supervised.timeout,false);assert.equal(supervised.not_started,false);assert.equal(supervised.reaped,true);assert.equal(supervised.error,null);assert.equal(supervised.sources_unchanged,true);assert.equal(supervised.engine_unchanged,true);assert.equal(supervised.captures_unchanged,true);assert.deepEqual(supervised.changed_sources,[]);assert.deepEqual(supervised.pin_errors,[]);uint(supervised.wall_ns,600e9);
  assert.deepEqual(Object.keys(supervised.files).sort(),[...files].map(f=>`${relative(bankDirectory,directory)}/${f}`).sort());for(const f of files)assert.deepEqual(supervised.files[`${relative(bankDirectory,directory)}/${f}`],pin(readFileSync(safeFile(directory,f))));
  return{schema_version:1,complete:true,semantic_certification:true,freeze_sha256:hash(freezeBytes),result_sha256:hash(resultBytes),raw_sha256:hash(raw),plan_sha256:hash(planBytes),engine:freeze.engine,counts,positive,zero:counts.generated_calls-positive,independently_replayed_instructions:retired,physical_files:files.size,journal_rows:journal.length,checkpoint_rows:checkpoints.length,observation_limit:'finite authored WORD memory CMOV, FLAGS/EA/RAM/owner proof; full frames only before/after each diagnostic sweep, per-read helper packets visible; no intermediate diagnostic full-arena, arbitrary helper side-effect rollback, exhaustive ISA, platform or performance claim'};
}
export function inspectIncomplete(directory){directory=resolve(directory);const files=readdirSync(directory).sort().map(file=>({file,...pin(readFileSync(safeFile(directory,file)))})),raw=files.find(r=>r.file==='arenas.bin');return{schema_version:1,complete:false,semantic_certification:false,files,available_full_frames:Math.floor((raw?.bytes??0)/SIZE),trailing_frame_bytes:(raw?.bytes??0)%SIZE,observation_limit:'uncertified physical inventory; no partial semantic replay'};}

function selfTest(){
  let assertions=0;const same=(a,b)=>{assert.deepEqual(a,b);assertions++;},reject=fn=>{assert.throws(fn);assertions++;};
  const masks=[0xffff0000,0xffff,0xaaaaaaaa,0x55555555,0xf0f0f0f0,0x0f0f0f0f,0xfafafafa,0x05050505,0xff00ff00,0x00ff00ff,0xcccccccc,0x33333333,0x00ffff00,0xff0000ff,0xf0fffff0,0x0f00000f];
  for(let cc=0;cc<16;cc++)for(let n=0;n<32;n++){const flags=2+[1,4,64,128,2048].reduce((v,b,i)=>v+(n>>i&1)*b,0);same(Number(predicate(cc,flags)),Math.floor(masks[cc]/2**n)%2);same(predicate(cc,flags),predicate(cc,flags|1040));}
  const setup=()=>{const c=fresh({id:1,owner:'replacement',entries:false,key:[17,19]});c.arena=authoredArena();c.ram.map(CODE,1,7);c.ram.write(CODE,literalBank());c.ram.protect(CODE,1,5);c.ram.map(DATA,1,3);c.ram.write(DATA,literalData());c.ram.protect(DATA,1,1);return c;};
  const op=decodeAt(Buffer.from('660f441b','hex'),0,CODE);same(op.source,{base:3,index:null,scale:1,displacement:0});
  for(const flags of [2,66]){const c=setup(),cpu=state(c.arena);cpu.registers[3]=DATA;cpu.flags=flags;const out=execute(c,op,cpu,null,[]);same(out.cpu.registers[3],flags===2?DATA:32769);same(out.cpu.flags,flags);same(c.arena.subarray(100,140),record('R3MH',40,[0,32769,0,0,0,2],2));}
  for(const[bits,detail]of[[0,2],[2,2],[4,2]]){const c=setup();c.ram.protect(DATA,1,bits);const cpu=state(c.arena);cpu.registers[3]=DATA;cpu.flags=2;const before={...cpu,registers:[...cpu.registers]};same(execute(c,op,cpu,null,[]),{fault:{reason:5,detail,address:DATA,access:1,length:2}});same(cpu,before);}
  {const c=setup(),cpu=state(c.arena);cpu.registers[3]=DATA+4095;same(execute(c,op,cpu,null,[]),{fault:{reason:5,detail:1,address:DATA+4096,access:1,length:2}});c.ram.map(DATA+4096,1,3);c.ram.write(DATA+4096,literalData());c.ram.protect(DATA+4096,1,1);same(execute(c,op,cpu,null,[]).cpu.pc,CODE+4);same(c.ram.read(DATA+4095,2).bytes,Buffer.from([194,1]));}
  {const c=setup(),cpu=state(c.arena);cpu.registers[3]=U32-1;same(execute(c,op,cpu,null,[]),{fault:{reason:5,detail:3,address:U32-1,access:1,length:2}});c.ram.map(U32-4096,1,3);c.ram.write(U32-4096,literalData());c.ram.protect(U32-4096,1,1);cpu.registers[3]=U32-2;same(execute(c,op,cpu,null,[]).cpu.pc,CODE+4);same(c.ram.read(U32-2,2).bytes,Buffer.from([121,194]));}
  for(const h of literalHelpers()){const c=setup(),cpu=state(c.arena);cpu.registers[3]=DATA;same(execute(c,op,cpu,h,[]),{fault:{reason:7,detail:h.id==='status'?3:2}});same(c.arena.subarray(100,140).toString('hex'),h.packet_hex);}
  for(const path of [0,1]){const c=setup(),pc=6144+path*64,regs=state(c.arena).registers;regs[0]=path?0xa1010000:0xa1018000;regs[1]=0xa2020001;regs[3]=DATA;record('R3ST',56,[...regs,pc,0xcd7]).copy(c.arena);const ins=new Map();for(const[start,len]of [[pc,23],[pc+25,2]]){let n=0;while(n<len){const o=decodeAt(literalBank(),start+n-CODE,start+n);ins.set(o.pc,o);n+=o.length;}}const u={instructions:ins,tokens:c.ram.snapshot([[pc,23],[pc+25,2]])};c.units.set('control',u);const before=Buffer.from(c.arena),out=generated(c,{channel:'direct',group:'control',budget:9});same(out.retired,8);same(out.reason,3);const want=Buffer.from(before);record('R3ST',56,[0xa1010100,0xa2020000,0xa3038001,DATA,0xa5058000,0xa6061234,0xa6061234,0xa8085555,pc+27,path?0x497:0xc16]).copy(want);record('R3EX',40,[3,8,0,0,0,0],2).copy(want,56);record('R3MH',40,[0,0x1234,0,0,0,2],2).copy(want,100);same(c.arena,want);before.copy(c.arena);for(const n of[1,3,4])same(generated(c,{channel:'direct',group:'control',budget:n}).retired,n);same(state(c.arena),state(want));same(generated(c,{channel:'direct',group:'control',budget:1}).retired,0);const bad=Buffer.from(want);bad[4363]^=1;reject(()=>assert.deepEqual(bad,want));}
  for(const entries of[false,true]){const c=setup(),a={name:entries?'compile_entries':'compile',args:[1]};c.arena.writeUInt32LE(6147,140);if(!entries)c.arena.writeUInt32LE(4,144);same(compileDescriptors(c,a).instructions.size,entries?6:1);a.args[0]=2;c.arena.writeUInt32LE(7936,entries?144:148);if(!entries)c.arena.writeUInt32LE(2,152);reject(()=>compileDescriptors(c,a));}
  for(const channel of['direct','dispatch'])for(const cancel of[0,1]){const c=setup(),o=decodeAt(Buffer.from('660f441b','hex'),0,CODE);c.units.set('one',{instructions:new Map([[CODE,o]]),tokens:c.ram.snapshot([[CODE,4]]),installed:true,id_low:3,id_high:7,slot:0});c.arena.writeUInt32LE(cancel,96);const before=Buffer.from(c.arena),out=generated(c,{channel,group:'one',budget:0});same(out.retired,0);same(out.reason,cancel?2:1);const want=Buffer.from(before);record('R3EX',40,[cancel?2:1,0,0,0,0,0],channel==='dispatch'?3:2).copy(want,56);same(c.arena,want);c.ram.protect(CODE,1,7);c.ram.write(CODE,Buffer.from([102]));const stale=Buffer.from(c.arena);same(generated(c,{channel,group:'one',budget:1}).status,channel==='dispatch'&&cancel?0:4);if(!(channel==='dispatch'&&cancel))same(c.arena,stale);c.closed=true;same(generated(c,{channel,group:'one',budget:1}).status,5);}
  for(const hex of['66','660f','660f44','66660f4403','67660f4403','f2660f4403'])reject(()=>decodeAt(Buffer.from(hex,'hex'),0,CODE));
  function signed(n){let value=BigInt.asIntN(32,BigInt(n));const bytes=[];for(;;){let byte=Number(value&127n);value>>=7n;const done=value===0n&&!(byte&64)||value===-1n&&Boolean(byte&64);if(!done)byte|=128;bytes.push(byte);if(done)return bytes;}}
  function leb(n){const b=[];do{let x=n%128;n=Math.floor(n/128);if(n)x|=128;b.push(x);}while(n);return b;}
  function boundBody(values,dispatch=false){const b=dispatch?[1,5,127]:[3,16,127,1,126,6,127];for(const n of values)b.push(65,...signed(n));b.push(32,0,32,1,32,3,16,0,34,dispatch?5:16,4,64,32,dispatch?5:16,15,11,dispatch?65:63,0,11);const section=[1,...leb(b.length),...b];return Buffer.from([0,97,115,109,1,0,0,0,10,...leb(section.length),...section]);}
  for(const owner of ['replacement','resident']){const spec={key:[17,19]},row={owner,target:'child',generation:7,id_low:11,id_high:13},values=owner==='replacement'?[17,19,7]:[17,19,11,13],b=boundBody(values);moduleBinding(b,row,spec);assertions++;reject(()=>moduleBinding(b,row,{key:[18,19]}));reject(()=>moduleBinding(b,{...row,generation:8,id_low:12},spec));const bad=Buffer.from(b);bad[12]=15;reject(()=>moduleBinding(bad,row,spec));}
  moduleBinding(boundBody([17,19],true),{owner:'resident',target:'dispatcher'},{key:[17,19]});assertions++;
  return{pure_assertions:assertions,engine_or_guest_execution:false};
}
function mutationTest(plan){
  const rejected=[];function reject(name,mutate){const copy=structuredClone(plan);mutate(copy);assert.throws(()=>validatePlan(copy),name);rejected.push(name);}
  validatePlan(plan);
  reject('raw source address byte',p=>{const b=Buffer.from(p.bank.hex,'hex');b[4]^=1;p.bank.hex=b.toString('hex');});
  reject('omit executed memory form',p=>p.bank.groups[0].blocks[0][1]--);
  reject('change declared cap census',p=>p.counts.frames--);
  reject('missing owner',p=>p.contexts.pop());
  reject('wrong owner key',p=>p.contexts[0].key[0]++);
  reject('inert helper wrong-width accepted packet',p=>{const b=Buffer.from(p.helper_cases[2].packet_hex,'hex');b.writeUInt32LE(2,36);p.helper_cases[2].packet_hex=b.toString('hex');});
  reject('false fault predicate made true',p=>{const a=p.actions.find(a=>a.kind==='call'&&a.label==='none false prefix fault');const seed=p.actions[a.id-1],b=Buffer.from(seed.hex,'hex');b.writeUInt32LE(b.readUInt32LE(52)|2048,52);seed.hex=b.toString('hex');});
  reject('truth FLAGS sentinel omitted',p=>{const a=p.actions.find(a=>a.kind==='call'&&a.label==='predicate truth'&&a.context===1&&p.actions[a.id-1].hex.slice(104,112)==='12000000');const seed=p.actions[a.id-1],b=Buffer.from(seed.hex,'hex');b.writeUInt32LE(2,52);seed.hex=b.toString('hex');});
  reject('old full32 alias address changed',p=>{const a=p.actions.find(a=>a.kind==='call'&&a.label==='both');const seed=p.actions[a.id-1],b=Buffer.from(seed.hex,'hex');b.writeUInt32LE(b.readUInt32LE(16)+1,16);seed.hex=b.toString('hex');});
  reject('overflow repair broadens writable CPU',p=>{const a=p.actions.find(a=>a.kind==='input'&&a.offset===28);a.offset=24;});
  reject('retry replays prefix NOP',p=>{const a=p.actions.find(a=>a.kind==='call'&&a.label.endsWith('unchanged retry'));a.planned_retired=1;});
  reject('successful top span becomes overflowing',p=>{const a=p.actions.find(a=>a.kind==='call'&&a.label==='last-two-byte read'&&p.actions[a.id-1].hex.slice(56,64)==='feffffff');const seed=p.actions[a.id-1],b=Buffer.from(seed.hex,'hex');b.writeUInt32LE(0xffffffff,28);seed.hex=b.toString('hex');});
  reject('closed call uses live-after evidence',p=>{const a=p.actions.find(a=>a.kind==='closed_call');a.kind='call';});
  return{mutations:rejected.length,rejected,engine_or_guest_execution:false};
}
if(process.argv[1]&&resolve(process.argv[1])===fileURLToPath(import.meta.url)){
  if(process.argv[2]==='--self-test')console.log(JSON.stringify(selfTest(),null,2));
  else if(process.argv[2]==='--mutations'){assert.equal(process.argv.length,4);console.log(JSON.stringify(mutationTest(JSON.parse(readFileSync(process.argv[3]))),null,2));}
  else if(process.argv[2]==='--plan'){assert.equal(process.argv.length,4);console.log(JSON.stringify(validatePlan(JSON.parse(readFileSync(process.argv[3]))),null,2));}
  else{assert.equal(process.argv.length,4,'usage: check.mjs OUTPUT FREEZE');const r=JSON.parse(readFileSync(join(resolve(process.argv[2]),'result.json')));const receipt=r.status==='complete'?checkCapsule(process.argv[2],process.argv[3]):inspectIncomplete(process.argv[2]);console.log(JSON.stringify(receipt,null,2));if(!receipt.complete)process.exitCode=2;}
}
