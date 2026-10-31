function cpuInstructionLimit(value) {
  if (!Number.isInteger(value) || value < 1 || value > 4096)
    throw new Error('invalid --cpu-instructions; expected an integer from 1 to 4096');
  return value;
}

export function engineRuntimeOptions(args) {
  const options = {};
  const switches = args.filter(value => value === '--cpu-instructions' || value.startsWith('--cpu-instructions='));
  if (switches.length > 1) throw new Error('duplicate --cpu-instructions switch');
  if (switches.some(value => value !== '--cpu-instructions'))
    throw new Error('--cpu-instructions requires a separate integer value');
  const at = args.indexOf('--cpu-instructions');
  if (at >= 0) {
    const value = args[at + 1];
    if (value === undefined || value.startsWith('--')) throw new Error('missing --cpu-instructions value');
    if (!/^\d+$/.test(value)) throw new Error('invalid --cpu-instructions value');
    options.cpuInstructions = cpuInstructionLimit(Number(value));
  }
  for (const [key, enabled, disabled, label] of [
    ['relaxedFpu', '--relaxed-fpu', '--strict-fpu', 'FPU'],
    ['fastPreset', '--fast-preset', '--no-fast-preset', 'preset'],
  ]) {
    if (args.includes(enabled) && args.includes(disabled)) throw new Error(`conflicting ${label} switches`);
    if (args.includes(enabled) || args.includes(disabled)) options[key] = args.includes(enabled);
  }
  return options;
}

export function engineRuntime(module, {relaxedFpu, fastPreset, cpuInstructions} = {}) {
  if (cpuInstructions !== undefined) cpuInstructionLimit(cpuInstructions);
  const external = WebAssembly.Module.imports(module).some(
    item => item.kind === 'memory' && item.module === 'env' && item.name === 'memory',
  );
  if (!external) {
    if (relaxedFpu || fastPreset) throw new Error('experimental CPU flags require an external CPU engine');
    const independent = WebAssembly.Module.exports(module).some(
      item => item.kind === 'function' && item.name === 'ring3_abi_v1_cpu_abi',
    );
    if (cpuInstructions !== undefined && !independent)
      throw new Error('--cpu-instructions requires the independent CPU backend');
    return {backend: independent ? 'independent' : 'core',
      ...(cpuInstructions !== undefined ? {execution: {budget: 50000, instructions: cpuInstructions}} : {})};
  }
  if (cpuInstructions !== undefined) throw new Error('--cpu-instructions requires the independent CPU backend');
  if (relaxedFpu === undefined) relaxedFpu = true;
  if (fastPreset === undefined) fastPreset = relaxedFpu;
  if (fastPreset && !relaxedFpu) throw new Error('--fast-preset requires --relaxed-fpu');
  return {backend: 'v86', v86: {relaxedFpu, fastPreset}};
}
