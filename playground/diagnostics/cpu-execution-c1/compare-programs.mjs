import * as weighted from "./compare-weighted.mjs";

export const ENTRY = 0x1000, EXIT = 0x1800, DATA = 0x4000, STACK = 0x8000, ESP = 0x8800;
export const names = ["weighted", "mixed-call-store", "long-fp-chain", "integer-memory"];
const word = n => [n & 255, n >>> 8 & 255, n >>> 16 & 255, n >>> 24 & 255];

export function program(name) {
  if (name === "weighted") return { name, code: weighted.code, operations: weighted.instructions,
    data: weighted.data, starts: weighted.starts, loopCount: weighted.loopCount, finalCount: weighted.finalCount,
    expectedData: weighted.expectedData, weighted: true };
  if (!names.includes(name)) throw new Error(`unknown authored program ${name}`);
  const operations = [], data = new Uint8Array(4096), d = new DataView(data.buffer);
  d.setFloat32(0, 2, true); d.setFloat32(4, 0.5, true); d.setFloat32(8, 1, true);
  if (name === "integer-memory") d.setUint32(0, 0x12345678, true);
  let pc = ENTRY;
  const add = (kind, bytes, fields = {}) => { operations.push({ pc, kind, bytes, ...fields }); pc += bytes.length; };
  const branch = (kind, opcode, target) => add(kind, [...opcode, ...word(target - (pc + opcode.length + 4))], { target });
  if (name === "mixed-call-store") {
    branch("call", [0xe8], 0x1200);
  } else if (name === "long-fp-chain") {
    add("fld", [0xd9, 0x03], { offset: 0 });
    for (let i = 0; i < 64; i++) add("fadd", [0xd8, 0x43, 0x04], { offset: 4 });
    add("fstp", [0xd9, 0x1f], { offset: 0 });
  } else {
    add("mov-load", [0x8b, 0x03]); add("add-eax", [0x83, 0xc0, 0x01]);
    add("mov-store", [0x89, 0x07]); add("inc-esi", [0x46]);
  }
  add("dec-ecx", [0x49]); branch("jnz", [0x0f, 0x85], ENTRY);
  add("exit", [0xff, 0xe2]);
  if (name === "mixed-call-store") {
    pc = 0x1200;
    for (let i = 0; i < 8; i++) {
      add("fld", [0xd9, 0x03], { offset: 0 });
      add("fmul", [0xd8, 0x4b, 0x04], { offset: 4 });
      add("fadd", [0xd8, 0x43, 0x08], { offset: 8 });
      add("fstp", [0xd9, 0x5f, i * 4], { offset: i * 4 });
    }
    add("inc-esi", [0x46]); add("ret", [0xc3]);
  }
  const code = new Uint8Array(Math.max(...operations.map(op => op.pc + op.bytes.length)) - ENTRY);
  for (const op of operations) code.set(op.bytes, op.pc - ENTRY);
  return { name, code, operations, starts: new Set(operations.map(op => op.pc)), data,
    loopCount: name === "mixed-call-store" ? 37 : name === "long-fp-chain" ? 68 : 6,
    finalCount: 1, weighted: false };
}

function parity(n) { n &= 255; let bits = 0; for (let i = 0; i < 8; i++) bits += n >>> i & 1; return bits % 2 === 0; }
function flags(value, before, operand, addition, carry) {
  value >>>= 0; before >>>= 0;
  const overflow = addition ? (~(before ^ operand) & (before ^ value)) >>> 31 : ((before ^ operand) & (before ^ value)) >>> 31;
  return 2 | Number(carry) | (parity(value) ? 4 : 0) | (((before ^ operand ^ value) & 16) ? 16 : 0) |
    (value === 0 ? 64 : 0) | (value >>> 31 ? 128 : 0) | (overflow ? 2048 : 0);
}

export function oracle(p, iterations, limit = Infinity, { cancelled = false, fault = false } = {}) {
  if (p.weighted) return null;
  const ram = p.data.slice(), stack = new Uint8Array(4096), dv = new DataView(ram.buffer), sv = new DataView(stack.buffer);
  const regs = [0x55555555, iterations, EXIT, DATA, ESP, 0, 0, fault ? 0x6000 : DATA + 0xa0];
  const ops = new Map(p.operations.map(op => [op.pc, op])), fp = Array(8).fill(0), tags = Array(8).fill(3);
  let pc = ENTRY, retired = 0, eflags = 2, top = 0, fip = 0, fdp = 0, fop = 0, faulted = false;
  while (!cancelled && pc !== EXIT && retired < limit) {
    const op = ops.get(pc); if (!op) throw new Error(`oracle source frontier ${pc.toString(16)}`);
    if ((op.kind === "fstp" || op.kind === "mov-store") && fault) { faulted = true; break; }
    let next = pc + op.bytes.length;
    if (op.kind === "call") { regs[4] -= 4; sv.setUint32(regs[4] - STACK, next, true); next = op.target; }
    else if (op.kind === "ret") { next = sv.getUint32(regs[4] - STACK, true); regs[4] += 4; }
    else if (op.kind === "jnz") { if (regs[1]) next = op.target; }
    else if (op.kind === "exit") next = EXIT;
    else if (op.kind === "dec-ecx") { const before = regs[1]; regs[1] = before - 1 >>> 0; eflags = flags(regs[1], before, 1, false, eflags & 1); }
    else if (op.kind === "inc-esi") { const before = regs[6]; regs[6] = before + 1 >>> 0; eflags = flags(regs[6], before, 1, true, eflags & 1); }
    else if (op.kind === "mov-load") regs[0] = dv.getUint32(0, true);
    else if (op.kind === "add-eax") { const before = regs[0]; regs[0] = before + 1 >>> 0; eflags = flags(regs[0], before, 1, true, before === 0xffffffff); }
    else if (op.kind === "mov-store") dv.setUint32(regs[7] - DATA, regs[0], true);
    else {
      fip = pc; fop = (op.bytes[0] & 7) << 8 | op.bytes[1];
      fdp = op.kind === "fstp" ? regs[7] + op.offset : DATA + op.offset;
      if (op.kind === "fld") { top = top + 7 & 7; fp[top] = dv.getFloat32(op.offset, true); tags[top] = fp[top] === 0 ? 1 : 0; }
      else if (op.kind === "fmul") fp[top] *= dv.getFloat32(op.offset, true);
      else if (op.kind === "fadd") fp[top] += dv.getFloat32(op.offset, true);
      else if (op.kind === "fstp") { dv.setFloat32(regs[7] - DATA + op.offset, fp[top], true); tags[top] = 3; top = top + 1 & 7; }
      else throw new Error(`oracle operation ${op.kind}`);
    }
    pc = next; retired++;
  }
  return { ram, stack, regs, pc, retired, eflags, top, fp, tags, fip, fdp, fop,
    reason: cancelled ? 2 : faulted ? 23 : Number.isFinite(limit) && retired >= limit ? 1 : pc === EXIT ? 3 : 1, faulted };
}

export function expectedFpRecord(initial, o, controlWord) {
  const result = initial.slice(4236, 4236 + 128), d = new DataView(result.buffer);
  d.setUint16(16, controlWord, true); d.setUint16(18, o.top << 11, true);
  d.setUint16(20, o.tags.reduce((v, tag, i) => v | tag << i * 2, 0), true);
  d.setUint16(22, o.fop, true); d.setUint32(24, o.fip, true); d.setUint32(28, o.fdp, true);
  if (o.fip !== 0) {
    const seed = new DataView(initial.buffer, initial.byteOffset, initial.byteLength);
    if (seed.getUint16(4524 + 18, true) !== 0x1b || seed.getUint16(4524 + 22, true) !== 0x23)
      throw new Error("authored default segment seed must have CS=0x1b and DS=0x23");
    d.setUint16(32, 0x1b, true); d.setUint16(34, 0x23, true);
  }
  for (let i = 0; i < 8; i++) {
    const value = o.fp[i], bytes = new ArrayBuffer(8), v = new DataView(bytes);
    v.setFloat64(0, value, true); const raw = v.getBigUint64(0, true);
    const exponent = Number(raw >> 52n & 2047n), significand = exponent ? ((raw & ((1n << 52n) - 1n)) | 1n << 52n) << 11n : 0n;
    d.setBigUint64(40 + i * 10, significand, true);
    d.setUint16(48 + i * 10, exponent ? exponent - 1023 + 16383 : 0, true);
  }
  return result;
}
