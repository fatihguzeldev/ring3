import assert from 'node:assert/strict';
import test from 'node:test';
import {engineRuntime} from './engine-runtime.mjs';

const core = new WebAssembly.Module(new Uint8Array([0,97,115,109,1,0,0,0]));
const external = new WebAssembly.Module(new Uint8Array([
  0,97,115,109,1,0,0,0,
  2,15,1,3,101,110,118,6,109,101,109,111,114,121,2,0,1,
]));

test('self-contained engines select the core backend', () => {
  assert.deepEqual(engineRuntime(core), {backend: 'core'});
});

test('an imported CPU memory selects v86 with strict FPU defaults', () => {
  assert.deepEqual(engineRuntime(external), {
    backend: 'v86', v86: {relaxedFpu: false, fastPreset: false},
  });
});

test('explicit experimental options reach the external CPU', () => {
  assert.deepEqual(engineRuntime(external, {relaxedFpu: true, fastPreset: true}), {
    backend: 'v86', v86: {relaxedFpu: true, fastPreset: true},
  });
});

test('the fast preset requires an explicit relaxed FPU opt-in', () => {
  assert.throws(() => engineRuntime(external, {fastPreset: true}), /--relaxed-fpu/);
});

test('external CPU flags reject a self-contained engine', () => {
  assert.throws(() => engineRuntime(core, {relaxedFpu: true}), /external CPU engine/);
});
