import assert from 'node:assert/strict';

export function detectionGuardName(resident) {
  assert.equal(typeof resident, 'boolean', 'declared resident binding');
  return resident ? 'guard_detect_resident' : 'guard_detect';
}

export function generatedImports({bound = true, resident = false, helpers = [], afterDetection = []} = {}) {
  assert.equal(typeof bound, 'boolean', 'declared bound module');
  const guard = detectionGuardName(resident);
  const reserved = new Set(['guard', 'guard_resident', 'guard_detect', 'guard_detect_resident', 'finish_exception_detection']);
  const names = new Set();
  for (const group of [helpers, afterDetection]) {
    assert.ok(Array.isArray(group), 'declared helper names');
    for (const name of group) {
      assert.ok(typeof name === 'string' && name.length > 0, 'nonempty helper name');
      assert.ok(!reserved.has(name), `binding owns ${name}`);
      assert.ok(!names.has(name), `unique helper ${name}`);
      names.add(name);
    }
  }
  return [
    {module: 'env', name: 'memory', kind: 'memory'},
    ...[...(bound ? [guard] : []), ...helpers,
      ...(bound ? ['finish_exception_detection'] : []), ...afterDetection]
      .map(name => ({module: 'ring3', name, kind: 'function'})),
  ];
}

export function attachDetectionApi(api, nativeExports) {
  const names = ['guard_detect', 'guard_detect_resident', 'finish_exception_detection'];
  const bindings = names.map(name => {
    const native = nativeExports[`ring3_abi_v1_${name}`];
    assert.equal(typeof native, 'function', `native detection export ${name}`);
    return [name, native];
  });
  Object.assign(api, Object.fromEntries(bindings));
  return api;
}
