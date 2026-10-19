import assert from 'node:assert/strict';

export function detectionGuardName(resident) {
  assert.equal(typeof resident, 'boolean', 'declared resident binding');
  return resident ? 'guard_detect_resident' : 'guard_detect';
}

export function generatedImports({bound = true, resident = false, helpers = [], afterDetection = [], read32View = bound && resident && Array.isArray(helpers) && helpers.includes('cpu_read32')} = {}) {
  assert.equal(typeof bound, 'boolean', 'declared bound module');
  assert.equal(typeof read32View, 'boolean', 'declared optional Read32 view');
  const guard = detectionGuardName(resident);
  const reserved = new Set(['guard', 'guard_resident', 'guard_detect', 'guard_detect_resident', 'finish_exception_detection', 'cpu_read32_view']);
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
  assert.ok(!read32View || (bound && resident && helpers.includes('cpu_read32')), 'Read32 view requires a resident ordinary read');
  return [
    {module: 'env', name: 'memory', kind: 'memory'},
    ...[...(bound ? [guard] : []), ...helpers,
      ...(bound ? ['finish_exception_detection'] : []), ...afterDetection,
      ...(read32View ? ['cpu_read32_view'] : [])]
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
  const view = nativeExports.ring3_abi_v1_cpu_read32_view;
  assert.ok(view === undefined || typeof view === 'function', 'native optional Read32 view export');
  bindings.push(['cpu_read32_view', view ?? (() => 0)]);
  Object.assign(api, Object.fromEntries(bindings));
  return api;
}
