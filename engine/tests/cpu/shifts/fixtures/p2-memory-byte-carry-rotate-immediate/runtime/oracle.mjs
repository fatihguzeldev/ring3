import assert from "node:assert/strict";

export function rotate(kind, value, raw, flags) {
  assert.ok(kind === "left" || kind === "right");
  assert.ok(Number.isInteger(value) && value >= 0 && value < 256);
  assert.ok(Number.isInteger(raw) && raw >= 0 && raw < 256);
  assert.ok(Number.isInteger(flags) && flags >= 0 && flags <= 0xffffffff);
  const q = raw % 32,
    r = q % 9;
  if (r === 0) return { value, flags, q, r, cf: flags % 2, of: Math.floor(flags / 0x800) % 2 };
  let bits = String(flags % 2) + value.toString(2).padStart(8, "0");
  for (let step = 0; step < r; step++)
    bits = kind === "left" ? bits.slice(1) + bits[0] : bits[8] + bits.slice(0, 8);
  const cf = Number(bits[0]),
    of = q === 1 ? Number(bits[1] !== (kind === "left" ? bits[0] : bits[2])) : 0;
  const retained = flags - (flags % 2) - (Math.floor(flags / 0x800) % 2) * 0x800;
  return {
    value: Number.parseInt(bits.slice(1), 2),
    flags: retained + cf + of * 0x800,
    q,
    r,
    cf,
    of,
  };
}

export const ANCHORS = [
  ["left", 0x81, 1, 2, 2, 0x803],
  ["left", 0x81, 1, 0xcd7, 3, 0xcd7],
  ["right", 0x80, 1, 2, 0x40, 0x802],
  ["right", 0x80, 1, 0xcd7, 0xc0, 0x4d6],
  ["left", 0x81, 10, 2, 2, 3],
  ["left", 0x81, 19, 2, 2, 3],
  ["left", 0x81, 28, 2, 2, 3],
  ["right", 0x80, 10, 2, 0x40, 2],
  ["right", 0x80, 19, 2, 0x40, 2],
  ["right", 0x80, 28, 2, 0x40, 2],
  ["left", 0x81, 8, 2, 0x40, 3],
  ["right", 0x80, 8, 2, 0, 3],
  ["left", 0x81, 9, 0xcd7, 0x81, 0xcd7],
  ["right", 0x80, 18, 0xcd6, 0x80, 0xcd6],
  ["left", 0x81, 27, 2, 0x81, 2],
  ["left", 0x81, 33, 2, 2, 0x803],
  ["right", 0x80, 32, 0xcd7, 0x80, 0xcd7],
  ["left", 10, 10, 0xcd6, 20, 0x4d6],
  ["right", 10, 10, 0xcd6, 5, 0x4d6],
  ["left", 0, 0, 0xcd7, 0, 0xcd7],
];
