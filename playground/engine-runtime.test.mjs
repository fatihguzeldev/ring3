import assert from 'node:assert/strict';
import test from 'node:test';
import {engineRuntime, engineRuntimeOptions} from './engine-runtime.mjs';

const core = new WebAssembly.Module(new Uint8Array([0,97,115,109,1,0,0,0]));
const external = new WebAssembly.Module(new Uint8Array([
  0,97,115,109,1,0,0,0,
  2,15,1,3,101,110,118,6,109,101,109,111,114,121,2,0,1,
]));
const independentName = [...new TextEncoder().encode('ring3_abi_v1_cpu_abi')];
const independent = new WebAssembly.Module(new Uint8Array([
  0,97,115,109,1,0,0,0, 1,5,1,0x60,0,1,0x7f, 3,2,1,0,
  7,independentName.length+4,1,independentName.length,...independentName,0,0,
  10,6,1,4,0,0x41,1,0x0b,
]));

test('self-contained engines select the core backend', () => {
  assert.deepEqual(engineRuntime(core), {backend: 'core'});
});

test('an independent CPU capability selects its normal self-contained runtime', () => {
  assert.deepEqual(engineRuntime(independent), {backend: 'independent'});
  assert.throws(() => engineRuntime(independent, {relaxedFpu: true}), /external CPU engine/);
});

test('an imported CPU memory selects v86 with the fast startup defaults', () => {
  assert.deepEqual(engineRuntime(external), {
    backend: 'v86', v86: {relaxedFpu: true, fastPreset: true},
  });
});

test('explicit experimental options reach the external CPU', () => {
  assert.deepEqual(engineRuntime(external, {relaxedFpu: true, fastPreset: true}), {
    backend: 'v86', v86: {relaxedFpu: true, fastPreset: true},
  });
});

test('an explicit strict FPU choice also disables the default fast preset', () => {
  assert.deepEqual(engineRuntime(external, {relaxedFpu: false}), {
    backend: 'v86', v86: {relaxedFpu: false, fastPreset: false},
  });
  assert.throws(() => engineRuntime(external, {relaxedFpu: false, fastPreset: true}), /requires.*relaxed/);
});

test('external CPU flags reject a self-contained engine', () => {
  assert.throws(() => engineRuntime(core, {relaxedFpu: true}), /external CPU engine/);
});

test('omitted CLI switches preserve the selected engine startup defaults', () => {
  assert.deepEqual(engineRuntimeOptions([]), {});
  assert.deepEqual(engineRuntime(external, engineRuntimeOptions([])), {
    backend: 'v86', v86: {relaxedFpu: true, fastPreset: true},
  });
  assert.deepEqual(engineRuntime(core, engineRuntimeOptions([])), {backend: 'core'});
});

test('CLI opt-outs disable only the requested policies', () => {
  for (const [args, expected] of [
    [['--no-fast-preset'], {relaxedFpu: true, fastPreset: false}],
    [['--strict-fpu'], {relaxedFpu: false, fastPreset: false}],
    [['--strict-fpu', '--no-fast-preset'], {relaxedFpu: false, fastPreset: false}],
    [['--relaxed-fpu', '--fast-preset'], {relaxedFpu: true, fastPreset: true}],
  ]) {
    assert.deepEqual(engineRuntime(external, engineRuntimeOptions(args)), {backend: 'v86', v86: expected});
  }
});

test('contradictory CPU switches fail instead of silently selecting a policy', () => {
  assert.throws(() => engineRuntimeOptions(['--relaxed-fpu', '--strict-fpu']), /conflicting.*FPU/);
  assert.throws(() => engineRuntimeOptions(['--fast-preset', '--no-fast-preset']), /conflicting.*preset/);
  assert.throws(() => engineRuntime(external, engineRuntimeOptions(['--strict-fpu', '--fast-preset'])), /requires.*relaxed/);
});

test('explicit CPU instruction limits reach only the independent launch configuration', () => {
  for (const instructions of [1, 32, 64, 128, 4096]) {
    const options = engineRuntimeOptions(['--cpu-instructions', String(instructions)]);
    assert.deepEqual(options, {cpuInstructions: instructions});
    assert.deepEqual(engineRuntime(independent, options), {
      backend: 'independent', execution: {budget: 50000, instructions},
    });
  }
});

test('omitted instruction limits preserve every backend default without execution overrides', () => {
  for (const module of [core, independent, external]) {
    const runtime = engineRuntime(module, engineRuntimeOptions([]));
    assert.equal(Object.hasOwn(runtime, 'execution'), false);
  }
});

test('CPU instruction switches reject missing invalid and duplicate values', () => {
  for (const args of [
    ['--cpu-instructions'], ['--cpu-instructions', '--strict-fpu'],
    ['--cpu-instructions', ''], ['--cpu-instructions', '0'],
    ['--cpu-instructions', '-1'], ['--cpu-instructions', '4097'],
    ['--cpu-instructions', '1.5'], ['--cpu-instructions', '1e2'],
    ['--cpu-instructions', '0x40'], ['--cpu-instructions', 'Infinity'],
    ['--cpu-instructions=64'],
  ]) assert.throws(() => engineRuntimeOptions(args), /cpu-instructions/);
  for (const args of [
    ['--cpu-instructions', '64', '--cpu-instructions', '128'],
    ['--cpu-instructions', '64', '--cpu-instructions=128'],
  ]) assert.throws(() => engineRuntimeOptions(args), /duplicate.*cpu-instructions/);
});

test('CPU instruction limits reject other engines and invalid programmatic policies', () => {
  for (const module of [core, external])
    assert.throws(() => engineRuntime(module, {cpuInstructions: 64}), /independent/);
  for (const value of [0, -1, 4097, 1.5, NaN, Infinity, '64', null])
    assert.throws(() => engineRuntime(independent, {cpuInstructions: value}), /cpu-instructions/);
});
