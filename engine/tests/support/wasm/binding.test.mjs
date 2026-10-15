import assert from 'node:assert/strict';
import test from 'node:test';
import {attachDetectionApi, detectionGuardName, generatedImports} from './binding.mjs';

const memory = {module: 'env', name: 'memory', kind: 'memory'};
const fn = name => ({module: 'ring3', name, kind: 'function'});

test('binding declares exact ordered detection imports without modifying helpers', () => {
  const helpers = Object.freeze(['cpu_read32', 'cpu_store32']);
  const afterDetection = Object.freeze(['cpu_fetch_fault']);
  assert.equal(detectionGuardName(false), 'guard_detect');
  assert.equal(detectionGuardName(true), 'guard_detect_resident');
  assert.deepEqual(generatedImports({helpers, afterDetection}), [memory,
    fn('guard_detect'), fn('cpu_read32'), fn('cpu_store32'),
    fn('finish_exception_detection'), fn('cpu_fetch_fault')]);
  assert.deepEqual(generatedImports({resident: true, helpers}), [memory,
    fn('guard_detect_resident'), fn('cpu_read32'), fn('cpu_store32'), fn('finish_exception_detection')]);
  assert.deepEqual(generatedImports({bound: false, helpers, afterDetection}), [memory,
    fn('cpu_read32'), fn('cpu_store32'), fn('cpu_fetch_fault')]);
  assert.deepEqual(generatedImports(), [memory, fn('guard_detect'), fn('finish_exception_detection')]);
});

test('binding rejects ambiguous or caller-owned guard declarations', () => {
  for (const options of [
    {bound: 1}, {resident: 0}, {helpers: 'read32'}, {afterDetection: null},
    {helpers: ['']}, {helpers: [null]}, {helpers: ['read32', 'read32']},
    {helpers: ['read32'], afterDetection: ['read32']},
    ...['guard', 'guard_resident', 'guard_detect', 'guard_detect_resident', 'finish_exception_detection']
      .map(name => ({helpers: [name]})),
  ]) assert.throws(() => generatedImports(options));
  assert.throws(() => detectionGuardName());
});

test('native bindings retain real references and raw diagnostic guards', () => {
  const rawGuard = () => 0, rawResident = () => 1;
  const epoch = () => 42n, residentEpoch = () => 43n, finish = () => 7;
  const api = {guard: rawGuard, guard_resident: rawResident};
  const exports = {ring3_abi_v1_guard_detect: epoch,
    ring3_abi_v1_guard_detect_resident: residentEpoch,
    ring3_abi_v1_finish_exception_detection: finish};
  assert.equal(attachDetectionApi(api, exports), api);
  assert.deepEqual(api, {guard: rawGuard, guard_resident: rawResident,
    guard_detect: epoch, guard_detect_resident: residentEpoch, finish_exception_detection: finish});
  assert.equal(api.guard_detect(), 42n);
  assert.equal(api.finish_exception_detection(), 7);
});

test('missing native exports cannot partially attach a detection API', () => {
  const names = ['ring3_abi_v1_guard_detect', 'ring3_abi_v1_guard_detect_resident', 'ring3_abi_v1_finish_exception_detection'];
  for (const missing of names) {
    const nativeExports = Object.fromEntries(names.map(name => [name, () => 1n]));
    delete nativeExports[missing];
    const rawGuard = () => 0, api = {guard: rawGuard};
    assert.throws(() => attachDetectionApi(api, nativeExports), /native detection export/);
    assert.deepEqual(api, {guard: rawGuard});
  }
});
