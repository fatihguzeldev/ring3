export function engineRuntime(module, {relaxedFpu = false, fastPreset = false} = {}) {
  const external = WebAssembly.Module.imports(module).some(
    item => item.kind === 'memory' && item.module === 'env' && item.name === 'memory',
  );
  if (!external) {
    if (relaxedFpu || fastPreset) throw new Error('experimental CPU flags require an external CPU engine');
    return {backend: 'core'};
  }
  if (fastPreset && !relaxedFpu) throw new Error('--fast-preset requires --relaxed-fpu');
  return {backend: 'v86', v86: {relaxedFpu, fastPreset}};
}
