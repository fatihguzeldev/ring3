const assert = {
  ok(value, message = "authored weighted-program invariant") { if (!value) throw new Error(message); },
  equal(actual, expected) { if (actual !== expected) throw new Error(`weighted invariant ${actual} != ${expected}`); },
};

export const entry = 0x1000, exit = 0x1800, dataAddress = 0x4000;
export const outputOffset = 0xa0, normalizedOffset = 0xd0, lengthOffset = 0xe0;
export const instructions = [];
let pc = entry;
function add(mnemonic, bytes) { instructions.push({ pc, mnemonic, bytes }); pc += bytes.length; }
function memory(mnemonic, opcode, operation, register, offset) {
  const short = offset >= -128 && offset <= 127;
  add(mnemonic, [opcode, (short ? 0x40 : 0x80) | operation << 3 | register,
    ...(short ? [offset & 255] : [offset & 255, offset >>> 8 & 255, offset >>> 16 & 255, offset >>> 24 & 255])]);
}

export const data = Uint8Array.from({ length: 4096 }, (_, index) => (index * 37 + 19) & 255);
const view = new DataView(data.buffer);
const write = (offset, values) => values.forEach((value, index) => view.setFloat32(offset + index * 4, value, true));
write(0, [1.173, -0.928, 0.617, 0.221, -0.731, 0.529]);
write(0x20, [0.313, -0.271, 1.019, -0.417, 0.893, 0.137, 0.721, 0.193, -0.563,
  0.913, -0.127, 0.071, 0.179, 0.831, -0.211, -0.093, 0.257, 0.947]);
write(0x70, [0.173, -0.219, 0.081]);
write(0x80, [0.37149]);
write(0x84, [0.137, -0.113, 0.097, 0.053, -0.071, 0.043]);
write(outputOffset, [0, 0, 0, 0, 0, 0]);
write(normalizedOffset, [0, 0, 0]);
write(lengthOffset, [0]);

for (let row = 0; row < 6; row++) {
  const vector = row < 3 ? 0 : 12;
  for (let column = 0; column < 3; column++) {
    memory(`fld input ${row}:${column}`, 0xd9, 0, 3, vector + column * 4);
    memory(`fmul matrix ${row}:${column}`, 0xd8, 1, 3, 0x20 + row * 12 + column * 4);
    if (column !== 0) add("faddp st1,st0", [0xde, 0xc1]);
  }
  if (row < 3) memory(`fadd translation ${row}`, 0xd8, 0, 3, 0x70 + row * 4);
  memory("fmul weight", 0xd8, 1, 3, 0x80);
  memory(`fld seed accumulator ${row}`, 0xd9, 0, 3, 0x84 + row * 4);
  add("faddp st1,st0", [0xde, 0xc1]);
  memory(`fstp output ${row}`, 0xd9, 3, 7, row * 4);
}
add("dec ecx", [0x49]);
const displacement = entry - (pc + 6);
add("jnz entry", [0x0f, 0x85, displacement & 255, displacement >>> 8 & 255,
  displacement >>> 16 & 255, displacement >>> 24 & 255]);
export const loopCount = instructions.length;
export const tail = pc;
for (let column = 0; column < 3; column++) {
  memory(`fld transformed normal ${column}`, 0xd9, 0, 7, 12 + column * 4);
  add("fmul st0,st0", [0xd8, 0xc8]);
  if (column !== 0) add("faddp st1,st0", [0xde, 0xc1]);
}
add("fsqrt", [0xd9, 0xfa]);
memory("fstp normal length", 0xd9, 3, 3, lengthOffset);
for (let column = 0; column < 3; column++) {
  memory(`fld transformed normal ${column}`, 0xd9, 0, 7, 12 + column * 4);
  memory("fdiv normal length", 0xd8, 6, 3, lengthOffset);
  memory(`fstp unit normal ${column}`, 0xd9, 3, 3, normalizedOffset + column * 4);
}
add("jmp edx", [0xff, 0xe2]);
export const finalCount = instructions.length - loopCount;
assert.equal(loopCount, 77);
assert.equal(finalCount, 20);
export const code = Uint8Array.from(instructions.flatMap(item => item.bytes));
export const starts = new Set(instructions.map(item => item.pc));

function rounded(value) {
  const rounded = Math.fround(value), bytes = new ArrayBuffer(4), v = new DataView(bytes);
  v.setFloat32(0, rounded, true);
  const word = v.getUint32(0, true), neighbors = [];
  for (const adjacent of [word - 1, word + 1]) {
    v.setUint32(0, adjacent >>> 0, true);
    neighbors.push(v.getFloat32(0, true));
  }
  // all operands/intermediates stay finite and below 8. fewer than 16 RN53
  // arithmetic operations have error below 1e-12; this margin proves the same
  // final f32 word for RN53 and RN64 without treating JS as an ext80 oracle.
  const margin = Math.min(...neighbors.map(next => Math.abs(value - (rounded + next) / 2)));
  assert.ok(Number.isFinite(rounded) && margin > 1e-11, "reference stays away from an f32 halfway boundary");
  return rounded;
}
export const expectedData = data.slice();
const expected = new DataView(expectedData.buffer);
for (let row = 0; row < 6; row++) {
  const vector = row < 3 ? 0 : 12;
  let value = 0;
  for (let column = 0; column < 3; column++) value += view.getFloat32(vector + column * 4, true) * view.getFloat32(0x20 + row * 12 + column * 4, true);
  if (row < 3) value += view.getFloat32(0x70 + row * 4, true);
  value = value * view.getFloat32(0x80, true) + view.getFloat32(0x84 + row * 4, true);
  expected.setFloat32(outputOffset + row * 4, rounded(value), true);
}
let squared = 0;
for (let column = 0; column < 3; column++) squared += expected.getFloat32(outputOffset + 12 + column * 4, true) ** 2;
const length = rounded(Math.sqrt(squared));
expected.setFloat32(lengthOffset, length, true);
for (let column = 0; column < 3; column++) expected.setFloat32(normalizedOffset + column * 4,
  rounded(expected.getFloat32(outputOffset + 12 + column * 4, true) / length), true);
