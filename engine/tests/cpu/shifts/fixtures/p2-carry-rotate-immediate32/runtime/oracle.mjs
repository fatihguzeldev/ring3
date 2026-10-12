import assert from "node:assert/strict";

export function rotate(kind, value, raw, flags) {
  assert.ok(kind === "left" || kind === "right");
  assert.ok(Number.isInteger(value) && value >= 0 && value <= 0xffffffff);
  assert.ok(Number.isInteger(raw) && raw >= 0 && raw <= 255);
  assert.ok(Number.isInteger(flags) && flags >= 0 && flags <= 0xffffffff);
  const q = raw % 32,
    oldCF = flags % 2;
  if (q === 0)
    return { value, flags, q, r: q, old_cf: oldCF, cf: oldCF, of: Math.floor(flags / 0x800) % 2 };
  let bits = String(oldCF) + value.toString(2).padStart(32, "0");
  for (let step = 0; step < q; step++)
    bits = kind === "left" ? bits.slice(1) + bits[0] : bits[32] + bits.slice(0, 32);
  const cf = Number(bits[0]),
    of = q === 1 ? Number(bits[1] !== (kind === "left" ? bits[0] : bits[2])) : 0;
  const retained = flags - oldCF - (Math.floor(flags / 0x800) % 2) * 0x800;
  return {
    value: Number.parseInt(bits.slice(1), 2),
    flags: retained + cf + of * 0x800,
    q,
    r: q,
    old_cf: oldCF,
    cf,
    of,
  };
}

export const ANCHORS = [
  ["left", 0x80000001, 1, 2, 2, 0x803],
  ["left", 0x80000001, 1, 0xcd7, 3, 0xcd7],
  ["right", 0x80000000, 1, 2, 0x40000000, 0x802],
  ["right", 0x80000000, 1, 0xcd7, 0xc0000000, 0x4d6],
  ["left", 0x80000001, 2, 2, 5, 2],
  ["left", 0x80000001, 10, 2, 0x500, 2],
  ["right", 0x80000000, 10, 2, 0x200000, 2],
  ["left", 0x80000001, 31, 2, 0xa0000000, 2],
  ["left", 0x80000001, 31, 0xcd7, 0xe0000000, 0x4d6],
  ["right", 0x80000001, 31, 2, 5, 2],
  ["right", 0x80000001, 31, 0xcd7, 7, 0x4d6],
  ["left", 0, 31, 0xcd7, 0x40000000, 0x4d6],
  ["right", 0, 31, 0xcd7, 2, 0x4d6],
  ["left", 0xffffffff, 31, 2, 0xbfffffff, 3],
  ["right", 0xffffffff, 31, 2, 0xfffffffd, 3],
  ["right", 0xffffffff, 31, 0xcd7, 0xffffffff, 0x4d7],
  ["left", 0x80000001, 33, 2, 2, 0x803],
  ["right", 0x80000001, 255, 2, 5, 2],
  ["left", 0x81234567, 0, 0xcd7, 0x81234567, 0xcd7],
  ["right", 0x92345678, 32, 0xcd6, 0x92345678, 0xcd6],
  ["left", 0x80000001, 1, 0x80000cd7, 3, 0x80000cd7],
  ["right", 0, 0, 0xfffffc02, 0, 0xfffffc02],
];
