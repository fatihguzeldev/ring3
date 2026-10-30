export function engineRuntimeOptions(args) {
  const options = {};
  for (const [key, enabled, disabled, label] of [
    ['relaxedFpu', '--relaxed-fpu', '--strict-fpu', 'FPU'],
    ['fastPreset', '--fast-preset', '--no-fast-preset', 'preset'],
  ]) {
    if (args.includes(enabled) && args.includes(disabled)) throw new Error(`conflicting ${label} switches`);
    if (args.includes(enabled) || args.includes(disabled)) options[key] = args.includes(enabled);
  }
  return options;
}

export function engineRuntime(module, {relaxedFpu, fastPreset} = {}) {
  const external = WebAssembly.Module.imports(module).some(
    item => item.kind === 'memory' && item.module === 'env' && item.name === 'memory',
  );
  if (!external) {
    if (relaxedFpu || fastPreset) throw new Error('experimental CPU flags require an external CPU engine');
    return {backend: 'core'};
  }
  if (relaxedFpu === undefined) relaxedFpu = true;
  if (fastPreset === undefined) fastPreset = relaxedFpu;
  if (fastPreset && !relaxedFpu) throw new Error('--fast-preset requires --relaxed-fpu');
  return {backend: 'v86', v86: {relaxedFpu, fastPreset}};
}
