// Literal task images and dynamic-save oracles, independent of the helper producer.
export const incomingRegisters = [0x11223344, 0x55667788, 0x99aabbcc, 0xddeeff00,
  0x7800, 0x2468ace0, 0xdeadbeef, 0xcafebabe];
export const incomingFlags = 0x240ed7, outgoingFlags = 0x254fd7;
export const selectors = [0x23, 0x1b, 0x23, 0x23, 0x3b, 0];
export const caches = [0xf33fffff, 0xf3bfffff, 0xf33fffff, 0xf33fffff, 0xf33fffff, 0];
export const ldt = [0x900000ff, 0x8200];

export function raw(base, limit, access, flags = 0) {
  return [(((base & 0xffff) << 16) | (limit & 0xffff)) >>> 0,
    ((base & 0xff000000) | (base >>> 16 & 255) | access << 8 | (limit & 0xf0000) | flags) >>> 0];
}
export function controls(record, {cr0 = 0x33, cr3 = 0xdeadb123, ldtr = 0x58, nmi = 2} = {}) {
  return record('R3CR', 40, [cr0, cr3, ldtr, 0x400, nmi, 0]);
}
export function taskImage({cs = 0x1b, ss = 0x23, flags = incomingFlags, pc = 0x2000, general = incomingRegisters} = {}) {
  const bytes = Buffer.alloc(104, 0xa5);
  bytes.writeUInt32LE(0xccccd000, 28); // supplied PG0 leaves live CR3 unchanged.
  bytes.writeUInt32LE(pc, 32); bytes.writeUInt32LE(flags, 36);
  general.forEach((value, index) => bytes.writeUInt32LE(value, 40 + 4 * index));
  [0x23, cs, ss, 0x23, 0x3b, 0].forEach((value, index) => bytes.writeUInt16LE(value, 72 + 4 * index));
  bytes.writeUInt16LE(0x50, 96); bytes.writeUInt16LE(0, 100); bytes.writeUInt16LE(104, 102);
  return bytes;
}
export function wordTaskImage({flags = 0x0ed7, pc = 0x2000, general = incomingRegisters} = {}) {
  const bytes = Buffer.alloc(44, 0xa5);
  bytes.writeUInt16LE(pc, 14); bytes.writeUInt16LE(flags, 16);
  general.forEach((value, index) => bytes.writeUInt16LE(value & 0xffff, 18 + 2 * index));
  [0x23, 0x1b, 0x23, 0x23].forEach((value, index) => bytes.writeUInt16LE(value, 34 + 2 * index));
  bytes.writeUInt16LE(0x50, 42); return bytes;
}
export function savedWordRAM(cloneRAM, writeRAM, before, currentBase, code, oldRegisters) {
  const want = cloneRAM(before), next = code === 'cf' ? 0x1001 : 0x1002;
  // Exactly fourteen low-word writes; the word TSS has no FS/GS, CR3 or high-word fields.
  const fields = [[14, next], [16, 0x0fd7],
    ...oldRegisters.map((value, index) => [18 + 2 * index, value & 0xffff]),
    ...selectors.slice(0, 4).map((value, index) => [34 + 2 * index, value])];
  for (const [offset, value] of fields) {
    const bytes = Buffer.alloc(2); bytes.writeUInt16LE(value);
    writeRAM(want, (currentBase + offset) >>> 0, bytes);
  }
  return want;
}
export function savedRAM(cloneRAM, writeRAM, before, currentBase, code, oldRegisters,
  {nextPC = code === 'cf' ? 0x1001 : 0x1002, flags = 0x240fd7} = {}) {
  const want = cloneRAM(before), next = nextPC;
  // All 26 dynamic 16-bit writes are literal; reserved selector halves/static TSS fields stay a5.
  const fields = [[32, next & 0xffff], [34, next >>> 16], [36, flags & 0xffff], [38, flags >>> 16],
    ...oldRegisters.flatMap((value, index) => [[40 + 4 * index, value & 0xffff], [42 + 4 * index, value >>> 16]]),
    ...selectors.map((value, index) => [72 + 4 * index, value])];
  for (const [offset, value] of fields) {
    const bytes = Buffer.alloc(2); bytes.writeUInt16LE(value);
    writeRAM(want, (currentBase + offset) >>> 0, bytes);
  }
  return want;
}
export function taskContext(record, {state, revision = 2n, loaded = 0x3f, knownLdt = true,
  selected = selectors, bases = [0, 0, 0, 0, 0, 0], cache = caches, fault = [0, 0]} = {}) {
  const bytes = record('R3TC', 184), canonical = Buffer.from(state.subarray(0, 56));
  const flags = canonical.readUInt32LE(52);
  canonical.writeUInt16LE(flags & 0x10000 ? 3 : flags & ~0xcd7 ? 2 : 1, 4);
  bytes.set(canonical, 16);
  bytes.writeUInt32LE(loaded, 72); bytes.writeUInt32LE(knownLdt ? 0x3f : 0x1f, 76);
  bytes.writeUInt32LE(fault[0], 80); bytes.writeUInt32LE(fault[1], 84);
  bytes.writeUInt32LE(0x3f03, 88); bytes.writeUInt32LE(0x50, 92);
  selected.forEach((value, index) => bytes.writeUInt32LE(value, 96 + 4 * index));
  bases.forEach((value, index) => bytes.writeUInt32LE(value, 120 + 4 * index));
  cache.forEach((value, index) => bytes.writeUInt32LE(value >>> 0, 144 + 4 * index));
  if (knownLdt) {bytes.writeUInt32LE(ldt[0], 168); bytes.writeUInt32LE(ldt[1], 172);}
  bytes.writeBigUInt64LE(revision, 176); return bytes;
}
