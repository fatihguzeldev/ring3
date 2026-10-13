import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {createArena, LEGACY_ARENA_SIZE, SEGMENT_BASES_RECORD_OFFSET} from './arena.mjs';

const [rustArenaPath] = process.argv.slice(2);
assert.ok(rustArenaPath, 'expected a current EngineInstance initial arena');
assert.deepEqual(createArena(), readFileSync(rustArenaPath),
  'fixture defaults match the complete current Rust ABI byte for byte');
const first = createArena(), second = createArena();
first.fill(0xa5);
assert.deepEqual(second, readFileSync(rustArenaPath), 'constructed arenas do not share storage');
assert.throws(() => createArena(4364), /current complete ABI arena/);

assert.deepEqual(createArena(LEGACY_ARENA_SIZE),
  readFileSync(rustArenaPath).subarray(0, LEGACY_ARENA_SIZE),
  'explicit legacy fixture prefix retains its original bytes');

assert.deepEqual(createArena(SEGMENT_BASES_RECORD_OFFSET),
  readFileSync(rustArenaPath).subarray(0, SEGMENT_BASES_RECORD_OFFSET),
  'the previous complete YMM arena remains an exact retained prefix');
