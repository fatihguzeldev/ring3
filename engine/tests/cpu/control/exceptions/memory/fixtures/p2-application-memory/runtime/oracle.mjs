import assert from 'node:assert/strict';
export const PC = 0x1000, TARGET = 0x2100, FLAGS = 0x2402d7, TRANSFER = 140, HELPER = 100;
export const word = values => {const b = Buffer.alloc(values.length * 4); values.forEach((v, i) => b.writeUInt32LE(v >>> 0, 4 * i)); return b;};
export function record(magic, size, values = [], version = 1) {
  const b = Buffer.alloc(size); b.write(magic); b.writeUInt16LE(version, 4); b.writeUInt16LE(1, 6);
  b.writeUInt32LE(size, 8); b.set(word(values), 16); return b;
}
export const call = (name, args, status = 0) => ({name,
  args: args.map(v => typeof v === 'bigint' ? v.toString(10) : v >>> 0),
  status: typeof status === 'bigint' ? status.toString(10) : status});
export const bound = (c, m, args) => [...(c.resident ? m.binding : []), ...args];
export function context(b, row, selector = 0x1b) {
  const sg = record('R3SG', 64, [], row.dsBase ? 3 : 1);
  [0x23, selector, 0x23, 0x23, 0x3b, 0].forEach((v, i) => sg.writeUInt16LE(v, 16 + 2 * i));
  if (row.dsBase) [0xf33fffff, 0xf3bfffff, 0xf33fffff, 0xf23000ff, 0xf33fffff, 0]
    .forEach((v, i) => sg.writeUInt32LE(v >>> 0, [28, 32, 36, 40, 44, 56][i]));
  b.set(sg, 4524); b.set(record('R3SB', 40, [0, 0, 0, row.dsBase ?? 0, 0, 0]), 4756); return b;
}
export function vector(b, register, value) {b.set(value.subarray(0, 16), 4396 + 16 * register); b.set(value.subarray(16), 4628 + 16 * register);}
export function seed(h, row) {
  const b = Buffer.from(h.initial), regs = [0x12345678, 0x23456789, 0x3456789a, 0x456789ab, 0x8000, 0x56789abc, 0x6789abcd, 0x789abcde];
  if (row.kind === 'gather') regs[0] = 0;
  b.set(record('R3ST', 56, [...regs, PC, FLAGS | 0x10000], 3)); b.writeUInt32LE(1, 12);
  for (let i = TRANSFER; i < 4236; i++) b[i] = (i * 17 + 3) & 255;
  for (let r = 0; r < 8; r++) vector(b, r, Buffer.from(Array.from({length: 32}, (_, i) => (0x40 + 19 * r + i) & 255)));
  if (row.kind === 'gather') {
    vector(b, 0, Buffer.from(Array.from({length: 32}, (_, i) => 0xc0 + i)));
    const mask = Buffer.alloc(32), index = Buffer.alloc(32); mask[row.width - 1] = 0x80; mask[2 * row.width - 1] = 0x80;
    index.set(word([0, 14, 0, 0])); vector(b, 2, mask); vector(b, 4, index);
  }
  return context(b, row);
}
export function gatherImages(row, input) {
  const destination = Buffer.concat([input.subarray(4396, 4412), input.subarray(4628, 4644)]);
  const mask = Buffer.alloc(32); mask.fill(0xff, row.width, 2 * row.width);
  destination.set(Buffer.from('1122334455667788', 'hex').subarray(0, row.width), 0);
  return {destination, mask, output: Buffer.concat([destination, mask])};
}
export function helper(row) {
  const width = row.width ?? 4, access = row.kind === 'store' || row.kind === 'exchange' ? 2 : 1;
  if (row.kind === 'gather') return record('R3GH', 40, [9, row.error, row.linear, 1, 1, 1], 2);
  if (row.mutation === 'forge') return record('R3MH', 40, [9, 0, 4, row.linear, 1, width], 15);
  if (row.success || row.mutation === 'downgrade') return record('R3MH', 40, [0, 0x44332211, 0, 0, 0, width], 10);
  return record('R3MH', 40, [9, 0, row.mutation === 'code' ? 5 : row.error, row.linear, access,
    row.mutation === 'width' ? 2 : width], 15);
}
export function stagedHelper(row, input) {
  const b = Buffer.from(input); b.set(helper({...row, mutation: undefined}), HELPER);
  if (row.kind === 'gather') {
    b.set(gatherImages(row, input).output, TRANSFER);
    b.set(record('R3GA', 24, [3, 0]), TRANSFER + 544);
  }
  return b;
}
export function sourceStop(row, input) {
  const b = Buffer.from(input), width = row.width ?? 4;
  b.writeUInt32LE(FLAGS, 52); b.writeUInt32LE(0, 12); b.set(helper(row), HELPER);
  let reason = 23, version = 12;
  let detail = row.error ?? 0, address = row.linear, access = row.kind === 'store' || row.kind === 'exchange' ? 2 : 1, length = width;
  if (row.protocol === 'shape' || row.protocol === 'pairing') {reason = 7; version = 6; detail = 2; address = access = length = 0;}
  if (row.protocol === 'witness') {reason = 23; version = 12; detail = row.mutation === 'code' ? 5 : 7;}
  b.set(record('R3EX', 40, [reason, 0, detail, address, access, length], version), 56);
  if (row.kind === 'gather') {
    const images = gatherImages(row, input); b.set(images.output, TRANSFER);
    b.set(record('R3GA', 24, [3, 0]), TRANSFER + 544);
    vector(b, 0, images.destination); vector(b, 2, images.mask);
  }
  return b;
}
export function entered(row, input) {return sourceStop(row, input);}
export function finishArgs(row) {
  const b = sourceStop(row, Buffer.alloc(4796));
  return [1n, ...[72, 80, 84, 88, 92].map(at => b.readUInt32LE(at)), 0, 0, 0, 0, 0, 0];
}
export function helperName(c, row) {
  const name = row.kind === 'read' ? 'cpu_read32' : row.kind === 'store' ? 'cpu_store32' : row.kind === 'exchange' ? 'cpu_exchange32' : 'avx_execute';
  return c.resident && row.kind !== 'read' ? name.replace(/32$/, '_resident32').replace(/^avx_execute$/, 'avx_execute_resident') : name;
}
export function sourceCalls(c, material, row) {
  const name = helperName(c, row), args = row.kind === 'gather' ? [0x01001400 | row.geometry, 0x1420, 0, 0, 0, FLAGS] :
    row.kind === 'read' ? [row.address] : [row.address, 0x12345678];
  const calls = [call(name, row.kind === 'read' ? args : bound(c, material, args), row.success ? 0 : 23)];
  if (row.protocol !== 'shape' && row.protocol !== 'pairing') calls.push(call('finish_exception_detection', finishArgs(row), row.protocol === 'witness' ? 7 : 0));
  return calls;
}
export function assertCapture(actual, expected, errors, label) {if (!actual.equals(expected)) errors.push(label);}
