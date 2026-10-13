import assert from 'node:assert/strict';

// retain the run body's binding contract while validating the optional pure segment callee.
export function generatedRunBody(bytes, {csBase = 0, default32 = true} = {}) {
  assert.ok(Number.isInteger(csBase) && csBase >= 0 && csBase <= 0xffffffff, 'declared retained CS base');
  assert.equal(typeof default32, 'boolean', 'declared retained code mode');
  assert.deepEqual([...bytes.subarray(0, 8)], [0, 97, 115, 109, 1, 0, 0, 0]);
  let p = 8;
  const leb = () => {
    let value = 0, shift = 0, byte;
    do { byte = bytes[p++]; assert.ok(byte !== undefined && shift <= 28); value += (byte & 127) * 2 ** shift; shift += 7; } while (byte & 128);
    return value;
  };
  const vector = () => {const length = leb(), result = [...bytes.subarray(p, p + length)]; p += length; return result;};
  const types = [], functions = [], bodies = [], importedTypes = [], guards = [];
  let globals;
  const signed = () => {
    let value = 0n, shift = 0n, byte;
    do {byte = bytes[p++]; assert.ok(byte !== undefined && shift < 35n); value |= BigInt(byte & 127) << shift; shift += 7n;} while (byte & 128);
    if (byte & 64) value -= 1n << shift;
    assert.ok(value >= -0x80000000n && value <= 0x7fffffffn, 'i32 global initializer');
    return Number(BigInt.asUintN(32, value));
  };
  const string = () => Buffer.from(vector()).toString();
  const arities = {guard: 6, guard_resident: 7, read8: 1, read16: 1, read32: 1, store8: 2, store16: 2, store32: 2, store_resident8: 6, store_resident16: 6, store_resident32: 6, cpu_read8: 1, cpu_read16: 1, cpu_read32: 1, cpu_store8: 2, cpu_store16: 2, cpu_store32: 2, cpu_store_resident8: 6, cpu_store_resident16: 6, cpu_store_resident32: 6};
  while (p < bytes.length) {
    const section = bytes[p++], size = leb(), end = p + size;
    assert.ok(end <= bytes.length);
    if (section === 1) {
      for (let n = leb(); n; n--) {assert.equal(bytes[p++], 0x60); types.push([vector(), vector()]);}
      assert.equal(p, end);
    } else if (section === 2) {
      for (let n = leb(); n; n--) {
        const module = string(), name = string(), kind = bytes[p++];
        if (kind === 2) {assert.equal(module, 'env'); assert.equal(name, 'memory'); assert.equal(leb(), 0); assert.equal(leb(), 1);}
        else {assert.equal(kind, 0); assert.equal(module, 'ring3'); assert.ok(Object.hasOwn(arities, name)); importedTypes.push([leb(), arities[name]]); if (name === 'guard' || name === 'guard_resident') guards.push(name);}
      }
      assert.equal(p, end);
    } else if (section === 3) {
      for (let n = leb(); n; n--) functions.push(leb());
      assert.equal(p, end);
    } else if (section === 6) {
      assert.equal(globals, undefined, 'one retained code-key global section');
      assert.equal(leb(), 3); globals = [];
      for (let n = 0; n < 3; n++) {
        assert.equal(bytes[p++], 0x7f); assert.equal(bytes[p++], 0, 'immutable code-key globals');
        assert.equal(bytes[p++], 0x41); globals.push(signed()); assert.equal(bytes[p++], 0x0b);
      }
      assert.equal(p, end);
    } else if (section === 10) {
      for (let n = leb(); n; n--) {const length = leb(); bodies.push(bytes.subarray(p, p + length)); p += length;}
      assert.equal(p, end);
    }
    p = end;
  }
  assert.ok(guards.length <= 1, 'one guard identifies a bound module');
  const bound = guards.length === 1;
  assert.deepEqual(globals, [Number(bound), csBase, Number(default32)], 'literal retained code key');
  const usedTypes = new Set(functions);
  for (const [type, arity] of importedTypes) {assert.deepEqual(types[type], [Array(arity).fill(0x7f), [0x7f]]); usedTypes.add(type);}
  assert.equal(usedTypes.size, types.length, 'every signature belongs to an imported or defined function');
  assert.ok(functions.length === 1 || functions.length === 2);
  assert.equal(functions.length, bodies.length);
  assert.equal(functions[0], 0);
  assert.deepEqual(types[0], [Array(4).fill(0x7f), [0x7f]]);
  const locals = body => {
    let at = 0;
    const read = () => {let n = 0, shift = 0, byte; do {byte = body[at++]; assert.ok(byte !== undefined && shift <= 28); n += (byte & 127) * 2 ** shift; shift += 7;} while (byte & 128); return n;};
    const result = []; for (let n = read(); n; n--) result.push([read(), body[at++]]);
    return {result, at};
  };
  assert.deepEqual(locals(bodies[0]).result, [[16, 0x7f], [1, 0x7e], [10, 0x7f], ...(bound ? [[1, 0x7f]] : [])]);
  if (functions.length === 2) {
    assert.equal(functions[1], types.length - 1);
    assert.deepEqual(types.at(-1), [Array(6).fill(0x7f), [0x7f]]);
    const body = bodies[1], shape = locals(body); assert.deepEqual(shape.result, [[3, 0x7f]]);
    let at = shape.at, depth = 0;
    const immediate = () => {let count = 0, byte; do {byte = body[at++]; assert.ok(byte !== undefined && ++count <= 5);} while (byte & 128);};
    while (at < body.length) {
      const opcode = body[at++];
      if ([0x20, 0x21, 0x22, 0x41].includes(opcode)) immediate();
      else if ([0x28, 0x2f].includes(opcode)) {immediate(); immediate();}
      else if (opcode === 0x04) {assert.ok([0x40, 0x7f].includes(body[at++])); depth++;}
      else if (opcode === 0x05) assert.ok(depth > 0);
      else if (opcode === 0x0b) {if (depth) depth--; else assert.equal(at, body.length);}
      else assert.ok([0x0f, 0x45, 0x46, 0x47, 0x49, 0x4b, 0x4d, 0x4f, 0x6a, 0x6b, 0x71, 0x72, 0x74].includes(opcode), 'segment callee has only pure integer/control/checked-load instructions');
    }
    assert.equal(depth, 0); assert.equal(body.at(-1), 0x0b);
  }
  return bodies[0];
}
