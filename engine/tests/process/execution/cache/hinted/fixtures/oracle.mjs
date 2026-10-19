import assert from "node:assert/strict";

export const ROOTS = [0x1000, 0x1400, 0x1800];
export const STARTS = [
  [0x1000, 0x1003, 0x1005, 0x100b, 0x100f, 0x1100],
  [0x1400, 0x1403, 0x1406, 0x1408, 0x140e, 0x1412, 0x1500],
  [0x1800, 0x1804, 0x1806, 0x180c, 0x1810, 0x1900],
];

// independent semantic table: the runner owns the separately authored instruction bytes.
const instructions = new Map([
  [0x1000, ["reg", 0, 1, 0x1003]],
  [0x1003, ["test", 0x1005]],
  [0x1005, ["jz", 0x1100, 0x100b]],
  [0x100b, ["memory", 4, 17, 0x100f]],
  [0x100f, ["jump", 0x1100]],
  [0x1100, ["indirect", 6]],
  [0x1400, ["reg", 5, 2, 0x1403]],
  [0x1403, ["memory", 0, 1, 0x1406]],
  [0x1406, ["test", 0x1408]],
  [0x1408, ["jz", 0x1500, 0x140e]],
  [0x140e, ["memory", 12, 33, 0x1412]],
  [0x1412, ["jump", 0x1500]],
  [0x1500, ["indirect", 1]],
  [0x1800, ["memory", 8, 3, 0x1804]],
  [0x1804, ["test", 0x1806]],
  [0x1806, ["jz", 0x1900, 0x180c]],
  [0x180c, ["memory", 16, 49, 0x1810]],
  [0x1810, ["jump", 0x1900]],
  [0x1900, ["indirect", 2]],
]);

export function initial(options = {}) {
  return {
    gpr: [5, 0x1800, 0x1000, options.fault ? 0xdead0000 : 0x8000, 0x8800, 4, 0x1403, options.edi ?? 0],
    pc: options.pc ?? 0x1000,
    flags: options.flags ?? 2,
  };
}

function parity(value) {
  let ones = 0;
  for (let bit = 0; bit < 8; bit++) ones += (value >>> bit) & 1;
  return ones % 2 === 0 ? 4 : 0;
}

function add(before, amount, flags) {
  const sum = before + amount, value = sum >>> 0;
  return [value, ((flags & ~0x8d5) | 2 | (sum > 0xffffffff ? 1 : 0) | parity(value)
    | (((before ^ amount ^ value) & 16) !== 0 ? 16 : 0)
    | (value === 0 ? 64 : 0) | ((value >>> 31) << 7)
    | (((~(before ^ amount) & (before ^ value)) >>> 31) << 11)) >>> 0];
}

export function ownerIndex(pc) {
  const index = STARTS.findIndex((starts) => starts.includes(pc));
  assert.ok(index >= 0, `authored instruction ${pc.toString(16)}`);
  return index;
}

export function expected(budget, options = {}, fused = false) {
  assert.ok(Number.isInteger(budget) && budget >= 0 && budget <= 10000);
  const input = initial(options), gpr = [...input.gpr], data = new Uint8Array(4096), view = new DataView(data.buffer);
  let pc = input.pc, flags = input.flags, retired = 0, reason = options.cancel ? 2 : 1;
  const children = [];
  if (!options.cancel && budget > 0) children.push({ owner: fused ? 0 : ownerIndex(pc), retired: 0 });
  while (!options.cancel && retired < budget) {
    const operation = instructions.get(pc);
    assert.ok(operation, `oracle pc ${pc.toString(16)}`);
    const [kind, a, b, next] = operation;
    if (kind === "reg") {
      [gpr[a], flags] = add(gpr[a], b, flags); pc = next;
    } else if (kind === "memory") {
      const address = (gpr[3] + a) >>> 0;
      if (address < 0x8000 || address > 0x8ffc) { reason = 23; break; }
      const offset = address - 0x8000;
      const result = add(view.getUint32(offset, true), b, flags);
      view.setUint32(offset, result[0], true); flags = result[1]; pc = next;
    } else if (kind === "test") {
      const value = gpr[7];
      flags = ((flags & ~0x8d7) | 2 | parity(value) | (value === 0 ? 64 : 0) | ((value >>> 31) << 7)) >>> 0;
      pc = a;
    } else if (kind === "jz") pc = (flags & 64) !== 0 ? a : b;
    else if (kind === "jump") pc = a;
    else if (kind === "indirect") pc = gpr[a];
    else assert.fail(`unknown oracle operation ${kind}`);
    retired++;
    children.at(-1).retired++;
    if (retired < budget && !fused && ownerIndex(pc) !== children.at(-1).owner)
      children.push({ owner: ownerIndex(pc), retired: 0 });
  }
  return { gpr, pc, flags, data, retired, reason, children };
}

export function packet(bytes, magic, size) {
  const v = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  assert.equal(bytes.length, size);
  assert.equal(Buffer.from(bytes.subarray(0, 4)).toString(), magic);
  assert.equal(v.getUint32(4, true), 0x10001);
  assert.equal(v.getUint32(8, true), size);
  assert.equal(v.getUint32(12, true), 0);
  return v;
}

export function validate(record, inputArena, codePage, owners, fused, options = {}) {
  const model = expected(record.budget, options, fused), state = Buffer.alloc(56);
  state.write("R3ST"); state.writeUInt16LE(3, 4); state.writeUInt16LE(1, 6); state.writeUInt32LE(56, 8);
  model.gpr.forEach((value, index) => state.writeUInt32LE(value >>> 0, 16 + 4 * index));
  state.writeUInt32LE(model.pc, 48); state.writeUInt32LE(model.flags, 52);
  assert.deepEqual(record.arena.subarray(0, 56), new Uint8Array(state), "literal full state");
  assert.deepEqual(record.arena.subarray(4236, 4588), inputArena.subarray(4236, 4588), "fp80/simd/segments unchanged");
  assert.deepEqual(record.arena.subarray(4612), inputArena.subarray(4612), "ymm/segment bases unchanged");
  assert.deepEqual(record.data, model.data, "complete data/stack page");
  assert.deepEqual(record.code, codePage, "complete guest code page");
  const frame = packet(record.frame, "R3EF", 32), receipt = packet(record.receipt, "R3ER", 64);
  const token = frame.getBigUint64(16, true), start = frame.getBigUint64(24, true);
  assert.ok(token > 0n);
  assert.equal(receipt.getUint32(36, true), model.retired);
  assert.equal(receipt.getUint32(44, true), model.reason);
  assert.ok(record.receipt.subarray(52).every((value) => value === 0));
  const childCount = receipt.getUint32(48, true) === 0 ? 0n : receipt.getBigUint64(16, true) - start;
  assert.equal(childCount, BigInt(model.children.length), "actual admitted child epochs");
  if (model.children.length === 0) {
    assert.equal(receipt.getUint32(48, true), 0);
    assert.equal(receipt.getBigUint64(24, true), 0n);
    assert.equal(receipt.getUint32(32, true), 0xffffffff);
    assert.equal(receipt.getUint32(40, true), 0);
  } else {
    const last = model.children.at(-1), owner = fused ? owners[0] : owners[last.owner];
    assert.equal(receipt.getBigUint64(24, true), owner.id);
    assert.equal(receipt.getUint32(32, true), owner.slot);
    assert.equal(receipt.getUint32(40, true), last.retired);
    assert.equal(receipt.getUint32(48, true), model.reason === 23 ? 3 : 1);
  }
  const exit = new DataView(record.outerExit.buffer, record.outerExit.byteOffset, 40);
  assert.equal(exit.getUint32(16, true), model.reason);
  assert.equal(exit.getUint32(20, true), model.retired);
  if (model.reason === 23) {
    assert.deepEqual([24, 28, 32, 36].map((at) => exit.getUint32(at, true)), [6, 0xdead0000, 2, 4]);
    assert.equal(model.retired, 4);
    assert.equal(model.pc, 0x1403);
  }
  const usageBefore = packet(record.usageBefore, "R3EU", 80), usageAfter = packet(record.usageAfter, "R3EU", 80);
  const delta = Array.from({ length: 8 }, (_, i) => usageAfter.getBigUint64(16 + 8 * i, true) - usageBefore.getBigUint64(16 + 8 * i, true));
  assert.ok(delta.every((value) => value >= 0n));
  if (record.mode === "compiled") {
    assert.equal(delta[4], childCount);
    assert.equal(delta[5], 0n);
    assert.equal(delta[0] + delta[1] + delta[2], BigInt(Math.max(0, model.children.length - 1)));
    assert.equal(delta[6], 0n);
    assert.equal(delta[7], 0n);
  } else assert.ok(delta.every((value) => value === 0n));
  return { retired: model.retired, reason: model.reason, childCount: childCount.toString(), gpr: model.gpr, pc: model.pc, flags: model.flags };
}

export function compareArchitecture(before, after) {
  for (const [start, end] of [[0, 56], [4236, 4588], [4612, 4796]])
    assert.deepEqual(before.arena.subarray(start, end), after.arena.subarray(start, end));
  assert.deepEqual(before.data, after.data);
  assert.deepEqual(before.code, after.code);
}
