export function makeKernel(kind, iterations) {
  if (!['integer', 'call'].includes(kind) || !Number.isSafeInteger(iterations) || iterations < 1 || iterations > 1048576) {
    throw new Error('expected an authored kernel and 1..1048576 iterations');
  }
  const code = new Uint8Array(4096), data = new Uint8Array(4096), stack = new Uint8Array(4096);
  let blocks, pc;
  if (kind === 'integer') {
    code.set([0x83, 0xc0, 3, 0x31, 0xc2, 0x83, 0x43, 0x0c, 1, 0x49, 0x75, 0xf4, 0x0f, 0x0b]);
    blocks = [[0x1000, 12], [0x100c, 2]]; pc = 0x100c;
  } else {
    code.set([0xe8, 0x3b, 0, 0, 0, 0x49, 0x75, 0xf8, 0x0f, 0x0b]);
    code.set([0x83, 0xc0, 3, 0x31, 0xc2, 0x83, 0x43, 0x0c, 1, 0xc3], 0x40);
    blocks = [[0x1000, 5], [0x1005, 3], [0x1008, 2], [0x1040, 10]]; pc = 0x1008;
  }
  let accumulator = 5, checksum = 7;
  for (let index = 0; index < iterations; index++) {
    accumulator = (accumulator + 3) >>> 0;
    checksum = (checksum ^ accumulator) >>> 0;
  }
  new DataView(data.buffer).setUint32(12, iterations, true);
  if (kind === 'call') new DataView(stack.buffer).setUint32(0x7fc, 0x1005, true);
  return {kind, iterations, code, blocks, pc, expectedData: data, expectedStack: stack,
    initialData: new Uint8Array(4096), initialStack: new Uint8Array(4096),
    initialRegs: [5, iterations, 7, 0x3000, 0x4800, 0, 0, 0],
    regs: [accumulator, 0, checksum, 0x3000, 0x4800, 0, 0, 0],
    flags: 0x46, retired: (kind === 'integer' ? 5 : 7) * iterations};
}
