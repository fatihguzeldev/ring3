import {FLAGS, TRANSFER, word, record, context} from './oracle.mjs';

export function segments(bytes, row, selector = 0x1b) {
  context(bytes, {}, selector);
  if (row.ssBase !== undefined) {
    const sg = record('R3SG', 64, [], 3);
    [0x23, selector, 0x23, 0x23, 0x3b, 0].forEach((v, i) => sg.writeUInt16LE(v, 16 + 2 * i));
    [0xf33fffff, 0xf3bfffff, row.ssCache, 0xf33fffff, 0xf33fffff, 0]
      .forEach((v, i) => sg.writeUInt32LE(v >>> 0, [28, 32, 36, 40, 44, 56][i]));
    bytes.set(sg, 4524); bytes.set(record('R3SB', 40, [0, 0, row.ssBase, 0, 0, 0]), 4756);
  }
  return bytes;
}

// FXSAVE checks a 512-byte envelope and publishes only its first 464 bytes.
export function fxImage(input) {
  const image = Buffer.from(input.subarray(TRANSFER, TRANSFER + 512)); image.fill(0, 0, 464);
  image.writeUInt16LE(input.readUInt16LE(4236 + 16), 0);
  image.writeUInt16LE(input.readUInt16LE(4236 + 18), 2);
  image.writeUInt32LE(input.readUInt32LE(4364 + 16), 24); image.writeUInt32LE(0xffff, 28);
  input.copy(image, 160, 4396, 4524); return image;
}

export function scratch(input, row, phase = 'final') {
  const b = Buffer.from(input);
  if (row.family === 'integer') b.set(word([1, FLAGS, input.readUInt32LE(16), input.readUInt32LE(24),
    input.readUInt32LE(28), input.readUInt32LE(20)]), TRANSFER);
  if (row.family === 'stack') input.copy(b, TRANSFER, 16, 48);
  if (row.family === 'store') input.copy(b, TRANSFER, 4396, 4412);
  if (row.family === 'fxsave') {b.set(fxImage(input), TRANSFER); b.set(word([0, FLAGS]), TRANSFER + 512);}
  if (row.family === 'mask') {
    input.copy(b, TRANSFER, 4396, 4412); b.set(word([0xff00, FLAGS]), TRANSFER + 512);
  }
  if (phase === 'prepare') b.set(record('R3MH', 40, [0, 0, 0, 0, 0, 0], 7), 100);
  else b.set(row.success ? record('R3MH', 40, [0, 0, 0, 0, 0, row.width], 7)
    : record('R3MH', 40, [9, 0, row.error, row.linear, row.access, row.width], 15), 100);
  return b;
}

export function sourceImage(input, row) {
  const b = scratch(input, row); b.writeUInt32LE(FLAGS, 52); b.writeUInt32LE(0, 12);
  b.set(record('R3EX', 40, row.success ? [1, 1, 0, 0, 0, 0]
    : [23, 0, row.error, row.linear, row.access, row.width], row.success ? 6 : 12), 56);
  if (row.success) b.writeUInt32LE(0x1000 + row.instructionLength, 48);
  return b;
}

export function entryImage(input, row) {return sourceImage(input, row);}

export function finishArguments(row) {
  return [1n, 23, row.error, row.linear, row.access, row.width, 0, 0, 0, 0, 0, 0];
}
