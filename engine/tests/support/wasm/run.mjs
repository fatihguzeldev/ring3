import assert from 'node:assert/strict';

// retain the run body's binding contract and the known segment/read32 callee composition.
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
  const types = [], functions = [], bodies = [], importedTypes = [], guards = [], finalizers = [];
  let globals;
  const signed = () => {
    let value = 0n, shift = 0n, byte;
    do {byte = bytes[p++]; assert.ok(byte !== undefined && shift < 35n); value |= BigInt(byte & 127) << shift; shift += 7n;} while (byte & 128);
    if (byte & 64) value -= 1n << shift;
    assert.ok(value >= -0x80000000n && value <= 0x7fffffffn, 'i32 global initializer');
    return Number(BigInt.asUintN(32, value));
  };
  const string = () => Buffer.from(vector()).toString();
  const arities = {guard: 6, guard_resident: 7, guard_detect: 6, guard_detect_resident: 7, finish_exception_detection: 12, cpu_read32_view: 1, read8: 1, read16: 1, read32: 1, store8: 2, store16: 2, store32: 2, store_resident8: 6, store_resident16: 6, store_resident32: 6, cpu_read8: 1, cpu_read16: 1, cpu_read32: 1, cpu_read_modify8: 1, cpu_read_modify16: 1, cpu_read_modify32: 1, cpu_store8: 2, cpu_store16: 2, cpu_store32: 2, cpu_store_resident8: 6, cpu_store_resident16: 6, cpu_store_resident32: 6};
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
        else {assert.equal(kind, 0); assert.equal(module, 'ring3'); assert.ok(Object.hasOwn(arities, name), `known generated import ${name}`); importedTypes.push([leb(), name]); if (name === 'guard' || name === 'guard_resident' || name === 'guard_detect' || name === 'guard_detect_resident') guards.push(name); if (name === 'finish_exception_detection') finalizers.push(name);}
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
  const detects = bound && guards[0].startsWith('guard_detect');
  const readView = importedTypes.some(([, name]) => name === 'cpu_read32_view');
  assert.ok(!readView || guards[0] === 'guard_detect_resident', 'only resident detection owns a Read32 view');
  assert.equal(finalizers.length, Number(detects), 'one finalizer accompanies a detection guard');
  assert.deepEqual(globals, [Number(bound), csBase, Number(default32)], 'literal retained code key');
  const usedTypes = new Set(functions);
  for (const [type, name] of importedTypes) {
    const parameters = Array(arities[name]).fill(0x7f);
    if (name === 'finish_exception_detection' || name === 'cpu_read32_view') parameters[0] = 0x7e;
    const result = name.startsWith('guard_detect') ? 0x7e : 0x7f;
    assert.deepEqual(types[type], [parameters, [result]], `exact signature for ${name}`);
    usedTypes.add(type);
  }
  assert.equal(usedTypes.size, types.length, 'every signature belongs to an imported or defined function');
  const signature = (index, inputs, outputs) =>
    JSON.stringify(types[index]) === JSON.stringify([Array(inputs).fill(0x7f), Array(outputs).fill(0x7f)]);
  let segment = null, read32 = null;
  for (let index = 1; index < functions.length; index++) {
    if (signature(functions[index], 6, 1)) {
      assert.equal(segment, null, 'one pure segment callee');
      assert.equal(index, 1, 'segment callee precedes the Read32 validator');
      segment = index;
    } else {
      assert.ok(signature(functions[index], 10, 7), 'only the declared Read32 validator may follow the segment callee');
      assert.equal(read32, null, 'one Read32 validator');
      assert.ok(detects && importedTypes.some(([, name]) => name === 'cpu_read32'), 'Read32 validator requires a bound ordinary read');
      assert.equal(index, functions.length - 1, 'Read32 validator is the final defined callee');
      read32 = index;
    }
  }
  assert.equal(functions.length, 1 + Number(segment !== null) + Number(read32 !== null));
  assert.equal(functions.length, bodies.length);
  assert.equal(functions[0], 0);
  assert.deepEqual(types[0], [Array(4).fill(0x7f), [0x7f]]);
  const locals = body => {
    let at = 0;
    const read = () => {let n = 0, shift = 0, byte; do {byte = body[at++]; assert.ok(byte !== undefined && shift <= 28); n += (byte & 127) * 2 ** shift; shift += 7;} while (byte & 128); return n;};
    const result = []; for (let n = read(); n; n--) result.push([read(), body[at++]]);
    return {result, at};
  };
  const runLocals = locals(bodies[0]).result;
  const expectedLocals = [[16, 0x7f], [1, 0x7e], [10, 0x7f], ...(bound ? [[1, 0x7f]] : []), ...(detects ? [[1, 0x7e], [6, 0x7f]] : []), ...(readView ? [[7, 0x7f], [1, 0x7e]] : [])];
  // only a resident view can add the single private direct-mov context marker.
  if (readView && runLocals.length === expectedLocals.length + 1) expectedLocals.push([1, 0x7f]);
  assert.deepEqual(runLocals, expectedLocals);
  if (segment !== null) {
    assert.equal(functions[segment], types.length - 1 - Number(detects) - Number(readView) - Number(read32 !== null));
    const body = bodies[segment], shape = locals(body); assert.deepEqual(shape.result, [[3, 0x7f]]);
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
  if (read32 !== null) {
    assert.equal(functions[read32], types.length - 1 - Number(readView), 'Read32 validator owns the final non-view type');
    const body = bodies[read32], shape = locals(body);
    assert.deepEqual(shape.result, [[17, 0x7f]], 'ten parameters plus seventeen canonical validator locals');
    const destinations = [25, 26, 15, 21, 22, 23, 24, 16, 17];
    const adapter = destinations.flatMap((local, index) => [0x20, index + 1, 0x21, local]);
    assert.deepEqual([...body.subarray(shape.at, shape.at + adapter.length)], adapter, 'exact canonical parameter adapter');
    const returned = [15, 21, 22, 23, 24, 16, 17].flatMap(local => [0x20, local]).concat(0x0b);
    assert.deepEqual([...body.subarray(-returned.length)], returned, 'exact canonical terminal/scratch return tuple');
  }
  return bodies[0];
}
