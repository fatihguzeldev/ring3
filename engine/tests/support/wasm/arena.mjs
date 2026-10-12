import assert from 'node:assert/strict';

export const ARENA_SIZE = 4612;

// Construct an input/expected arena once; never normalize an observed arena.
// Individual fixtures may replace these defaults with their explicit CPU/FP seeds.
export function createArena(size = ARENA_SIZE) {
  assert.equal(size, ARENA_SIZE, 'fixture uses the current complete ABI arena');
  const arena = Buffer.alloc(ARENA_SIZE);
  const header = (offset, magic, length) => {
    arena.write(magic, offset, 4, 'ascii');
    arena.writeUInt16LE(1, offset + 4);
    arena.writeUInt16LE(1, offset + 6);
    arena.writeUInt32LE(length, offset + 8);
  };
  header(0, 'R3ST', 56);
  arena.writeUInt32LE(2, 52);
  header(56, 'R3EX', 40);
  arena.writeUInt32LE(1, 72);
  header(100, 'R3MH', 40);
  header(4236, 'R3FP', 128);
  arena.writeUInt16LE(0x037f, 4252);
  arena.writeUInt16LE(0xffff, 4256);
  header(4364, 'R3SV', 160);
  arena.writeUInt32LE(0x1f80, 4380);
  header(4524, 'R3SG', 64);
  [0x23, 0x1b, 0x23, 0x23, 0x3b, 0].forEach((selector, index) =>
    arena.writeUInt16LE(selector, 4540 + index * 2));
  header(4588, 'R3WK', 24);
  return arena;
}
