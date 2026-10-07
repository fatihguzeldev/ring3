const defaultSeed = 0x6d2b79f5;
const modulus = 4294967296n;

export function generate(seed = defaultSeed, count = 256) {
  let state = BigInt.asUintN(32, BigInt(seed));
  const values = [];

  for (let index = 0; index < count; index += 1) {
    state ^= (state * 8192n) % modulus;
    state ^= state / 131072n;
    state ^= (state * 32n) % modulus;
    values.push(Number(state));
  }

  return values;
}

export function sorted(seed = defaultSeed, count = 256) {
  return generate(seed, count).sort((left, right) => left - right);
}

export function checksum(values) {
  let accumulator = 0n;

  for (const value of values) {
    const rotated = (accumulator * 32n) % modulus + accumulator / 134217728n;
    accumulator = rotated ^ BigInt.asUintN(32, BigInt(value));
  }

  return Number(accumulator);
}

export function batch(seed = defaultSeed, count = 256) {
  const input = generate(seed, count);
  const ordered = [...input].sort((left, right) => left - right);
  return { input, sorted: ordered, checksum: checksum(ordered) };
}
