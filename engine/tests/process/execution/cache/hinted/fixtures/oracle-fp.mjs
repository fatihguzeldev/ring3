import assert from "node:assert/strict";
import { packet, compareArchitecture } from "./oracle.mjs";
export { packet, compareArchitecture };

export const ROOTS = [0x1000, 0x1400, 0x1800];
export const STARTS = [
  [0x1000, 0x1003, 0x1006, 0x1008, 0x100e, 0x1012, 0x1100],
  [0x1400, 0x1403, 0x1406, 0x1409, 0x140b, 0x1411, 0x1415, 0x1500],
  [0x1800, 0x1803, 0x1806, 0x180a, 0x180c, 0x1812, 0x1816, 0x1900],
];
const operations = new Map([
  [0x1000, ["reg", 0, 1, 0x1003]],
  [0x1003, ["fld", 16, 0x0143, 0x1006]],
  [0x1006, ["test", 0x1008]], [0x1008, ["jz", 0x1100, 0x100e]],
  [0x100e, ["memory", 4, 17, 0x1012]], [0x1012, ["jump", 0x1100]], [0x1100, ["indirect", 6]],
  [0x1400, ["reg", 5, 2, 0x1403]],
  [0x1403, ["fadd", 20, 0x0043, 0x1406]],
  [0x1406, ["memory", 0, 1, 0x1409]], [0x1409, ["test", 0x140b]], [0x140b, ["jz", 0x1500, 0x1411]],
  [0x1411, ["memory", 12, 33, 0x1415]], [0x1415, ["jump", 0x1500]], [0x1500, ["indirect", 1]],
  [0x1800, ["fmul", 24, 0x004b, 0x1803]], [0x1803, ["fstp", 28, 0x015b, 0x1806]],
  [0x1806, ["memory", 8, 3, 0x180a]], [0x180a, ["test", 0x180c]], [0x180c, ["jz", 0x1900, 0x1812]],
  [0x1812, ["memory", 32, 49, 0x1816]], [0x1816, ["jump", 0x1900]], [0x1900, ["indirect", 2]],
]);
const values = {
  onePointFive: [0xc000000000000000n, 0x3fff],
  four: [0x8000000000000000n, 0x4001],
  eight: [0x8000000000000000n, 0x4002],
};

export function initial(options = {}) {
  return { gpr: [5, 0x1800, 0x1000, options.ebx ?? 0x8000, 0x8800, 4, 0x1403, options.edi ?? 0],
    pc: options.pc ?? 0x1000, flags: options.flags ?? 2 };
}

export function initialData(options = {}) {
  const data = new Uint8Array(4096), view = new DataView(data.buffer), ebx = initial(options).gpr[3];
  for (const base of [0x8000, ebx])
    for (const [offset, bits] of [[16, 0x3fc00000], [20, 0x40200000], [24, 0x40000000]]) {
      const address = (base + offset) >>> 0;
      if (address >= 0x8000 && address <= 0x8ffc) view.setUint32(address - 0x8000, bits, true);
    }
  return data;
}

function fpState(options) {
  const seed = options.fpSeed ?? null;
  assert.ok(seed === null || Object.hasOwn(values, seed));
  return { live: seed !== null, physical: seed, fip: 0, fdp: 0, fop: 0, selectors: false };
}

function encodeFp(template, options, fp) {
  const bytes = template.slice(4236, 4364), view = new DataView(bytes.buffer);
  assert.ok([0x007f, 0x027f, 0x037f].includes(options.cw));
  view.setUint16(16, options.cw, true);
  view.setUint16(18, fp.live ? 0x3800 : 0, true);
  view.setUint16(20, fp.live ? 0x3fff : 0xffff, true);
  view.setUint16(22, fp.fop, true); view.setUint32(24, fp.fip, true); view.setUint32(28, fp.fdp, true);
  view.setUint16(32, fp.selectors ? 0x1b : 0, true); view.setUint16(34, fp.selectors ? 0x23 : 0, true);
  if (fp.physical !== null) {
    const [significand, exponent] = values[fp.physical];
    view.setBigUint64(110, significand, true); view.setUint16(118, exponent, true);
  }
  return bytes;
}

export function initialFp(template, options) { return encodeFp(template, options, fpState(options)); }

function parity(value) {
  let ones = 0;
  for (let bit = 0; bit < 8; bit++) ones += (value >>> bit) & 1;
  return ones % 2 === 0 ? 4 : 0;
}
function add(before, amount, flags) {
  const sum = before + amount, value = sum >>> 0;
  return [value, ((flags & ~0x8d5) | 2 | (sum > 0xffffffff ? 1 : 0) | parity(value)
    | (((before ^ amount ^ value) & 16) !== 0 ? 16 : 0) | (value === 0 ? 64 : 0)
    | ((value >>> 31) << 7) | (((~(before ^ amount) & (before ^ value)) >>> 31) << 11)) >>> 0];
}
function ownerIndex(pc) {
  const index = STARTS.findIndex((starts) => starts.includes(pc));
  assert.ok(index >= 0, `authored fp pc ${pc.toString(16)}`); return index;
}

export function expected(budget, options, fused) {
  assert.ok(Number.isInteger(budget) && budget >= 0 && budget <= 10000);
  const input = initial(options), gpr = [...input.gpr], data = initialData(options), view = new DataView(data.buffer), fp = fpState(options);
  let pc = input.pc, flags = input.flags, retired = 0, reason = options.cancel ? 2 : 1, fault = null;
  const children = [];
  if (!options.cancel && budget > 0) children.push({ owner: fused ? 0 : ownerIndex(pc), retired: 0 });
  while (!options.cancel && retired < budget) {
    const operation = operations.get(pc);
    assert.ok(operation, `fp oracle pc ${pc.toString(16)}`);
    const [kind, a, b, next] = operation;
    if (["fld", "fadd", "fmul", "fstp", "memory"].includes(kind)) {
      const address = (gpr[3] + a) >>> 0, write = kind === "fstp" || kind === "memory";
      if (address < 0x8000 || address > 0x8ffc) { reason = 23; fault = [write ? 6 : 4, address, write ? 2 : 1, 4]; break; }
      const offset = address - 0x8000;
      if (kind === "memory") {
        const result = add(view.getUint32(offset, true), b, flags); view.setUint32(offset, result[0], true); flags = result[1];
      } else {
        // this finite oracle uses exact dyadic identities, never an approximate fp backend.
        if (kind === "fld") { assert.equal(fp.live, false); assert.equal(view.getUint32(offset, true), 0x3fc00000); fp.live = true; fp.physical = "onePointFive"; }
        if (kind === "fadd") { assert.ok(fp.live); assert.equal(fp.physical, "onePointFive"); assert.equal(view.getUint32(offset, true), 0x40200000); fp.physical = "four"; }
        if (kind === "fmul") { assert.ok(fp.live); assert.equal(fp.physical, "four"); assert.equal(view.getUint32(offset, true), 0x40000000); fp.physical = "eight"; }
        if (kind === "fstp") { assert.ok(fp.live); assert.equal(fp.physical, "eight"); view.setUint32(offset, 0x41000000, true); fp.live = false; }
        fp.fip = pc; fp.fdp = address; fp.fop = b; fp.selectors = true;
      }
      pc = next;
    } else if (kind === "reg") { [gpr[a], flags] = add(gpr[a], b, flags); pc = next; }
    else if (kind === "test") { const value = gpr[7]; flags = ((flags & ~0x8d7) | 2 | parity(value) | (value === 0 ? 64 : 0) | ((value >>> 31) << 7)) >>> 0; pc = a; }
    else if (kind === "jz") pc = (flags & 64) !== 0 ? a : b;
    else if (kind === "jump") pc = a;
    else if (kind === "indirect") pc = gpr[a];
    else assert.fail(`unknown fp oracle operation ${kind}`);
    retired++; children.at(-1).retired++;
    if (retired < budget && !fused && ownerIndex(pc) !== children.at(-1).owner) children.push({ owner: ownerIndex(pc), retired: 0 });
  }
  return { gpr, pc, flags, data, retired, reason, children, fp, fault };
}

export function validate(record, template, codePage, owners, fused, options) {
  const model = expected(record.budget, options, fused), state = Buffer.alloc(56);
  state.write("R3ST"); state.writeUInt16LE(3, 4); state.writeUInt16LE(1, 6); state.writeUInt32LE(56, 8);
  model.gpr.forEach((value, index) => state.writeUInt32LE(value >>> 0, 16 + 4 * index));
  state.writeUInt32LE(model.pc, 48); state.writeUInt32LE(model.flags, 52);
  assert.deepEqual(record.arena.subarray(0, 56), new Uint8Array(state), "literal full fp-kernel gpr/flags/state");
  assert.deepEqual(record.arena.subarray(4236, 4364), encodeFp(template, options, model.fp), "all 128 fp80 bytes including stale payload and pointers");
  assert.deepEqual(record.arena.subarray(4364, 4588), template.subarray(4364, 4588));
  assert.deepEqual(record.arena.subarray(4612), template.subarray(4612));
  assert.deepEqual(record.data, model.data); assert.deepEqual(record.code, codePage);
  const frame = packet(record.frame, "R3EF", 32), receipt = packet(record.receipt, "R3ER", 64), kind = receipt.getUint32(48, true);
  const count = kind === 0 ? 0n : receipt.getBigUint64(16, true) - frame.getBigUint64(24, true);
  assert.ok(frame.getBigUint64(16, true) > 0n);
  assert.equal(count, BigInt(model.children.length));
  assert.equal(receipt.getUint32(36, true), model.retired); assert.equal(receipt.getUint32(44, true), model.reason);
  assert.ok(record.receipt.subarray(52).every((value) => value === 0));
  if (model.children.length === 0) {
    assert.equal(kind, 0); assert.equal(receipt.getBigUint64(24, true), 0n); assert.equal(receipt.getUint32(32, true), 0xffffffff); assert.equal(receipt.getUint32(40, true), 0);
  } else {
    const last = model.children.at(-1), owner = fused ? owners[0] : owners[last.owner];
    assert.equal(kind, model.reason === 23 ? 3 : 1); assert.equal(receipt.getBigUint64(24, true), owner.id);
    assert.equal(receipt.getUint32(32, true), owner.slot); assert.equal(receipt.getUint32(40, true), last.retired);
  }
  const exit = new DataView(record.outerExit.buffer, record.outerExit.byteOffset, 40);
  assert.equal(exit.getUint32(16, true), model.reason); assert.equal(exit.getUint32(20, true), model.retired);
  if (model.fault !== null) assert.deepEqual([24, 28, 32, 36].map((at) => exit.getUint32(at, true)), model.fault);
  const a = packet(record.usageBefore, "R3EU", 80), b = packet(record.usageAfter, "R3EU", 80);
  const delta = Array.from({ length: 8 }, (_, i) => b.getBigUint64(16 + 8 * i, true) - a.getBigUint64(16 + 8 * i, true));
  assert.ok(delta.every((value) => value >= 0n));
  if (record.mode === "compiled") {
    assert.equal(delta[4], count); assert.equal(delta[5], 0n);
    assert.equal(delta[0] + delta[1] + delta[2], BigInt(Math.max(0, model.children.length - 1)));
    if (!fused && !options.cancel && !model.fault && options.pc === undefined && record.budget >= 16) assert.ok(delta[6] > 0n);
    assert.equal(delta[7], 0n);
  } else assert.ok(delta.every((value) => value === 0n));
  return { retired: model.retired, reason: model.reason, childCount: count.toString(), gpr: model.gpr, pc: model.pc, flags: model.flags, fp: { ...model.fp }, cw: options.cw };
}
