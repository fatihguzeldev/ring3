import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {arena, checkContext, checkImports, seedFlatCodeContext, SIZE} from './checks.mjs';

const bytes = readFileSync(0); let offset = 0, contexts = 0, runs = 0;
while (offset < bytes.length) {
  const resident = bytes[offset++] !== 0, entries = bytes[offset++] !== 0;
  const instructionLength = bytes[offset++], length = bytes.readUInt32LE(offset); offset += 4;
  const module = new WebAssembly.Module(bytes.subarray(offset, offset + length)); offset += length;
  checkImports(module, resident);
  const memory = new WebAssembly.Memory({initial: 1}), context = {memory, base: 0, label: `${resident ? 'resident' : 'replacement'}-${entries ? 'entry' : 'extent'}-${instructionLength}`};
  seedFlatCodeContext(context); let guards = 0;
  context.calls = [];
  const logged = value => typeof value === 'bigint' ? value.toString(10) : value >>> 0;
  const guard = (...args) => {
    const status = BigInt(++guards);
    context.calls.push({name: resident ? 'guard_detect_resident' : 'guard_detect', args: args.map(logged), status: logged(status)});
    return status;
  };
  const finish = (...args) => {
    context.calls.push({name: 'finish_exception_detection', args: args.map(logged), status: 0}); return 0;
  };
  context.run = new WebAssembly.Instance(module, {env: {memory}, ring3: {
    guard_detect: guard, guard_detect_resident: guard, finish_exception_detection: finish,
  }}).exports.run;
  const count = checkContext(context, instructionLength);
  assert.equal(guards, count, 'each native codegen run checks its bound identity once');
  assert.equal(arena(context).length, SIZE); runs += count; contexts++;
}
assert.equal(offset, bytes.length); assert.equal(contexts, 8);
console.log(JSON.stringify({contexts, runs, oracle: 'declared software CPU profile'}));
