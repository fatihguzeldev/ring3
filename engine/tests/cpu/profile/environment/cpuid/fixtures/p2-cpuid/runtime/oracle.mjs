// result order follows the x86 state record: eax, ecx, edx, ebx.
const vendor = [0x0d, 0x20656e69, 0x676e4533, 0x676e6952];
const features = [0x600, 0x1ed80203, 0x07888111, 0x800];
const structured = [0, 0, 0, 0x108];
// x87, sse and avx are enabled; standard-format extent includes the full avx component.
const xstate = [7, 832, 0, 832];
const upper = [256, 0, 0, 576];
const extendedLimit = [0x80000001, 0, 0, 0];
const extendedFeatures = [0, 0x20, 0x08000000, 0];
const zero = [0, 0, 0, 0];

export const queries = [
  ...[0, 0xffffffff].map(subleaf => ({leaf: 0, subleaf, result: vendor})),
  ...[0, 0xffffffff].map(subleaf => ({leaf: 1, subleaf, result: features})),
  {leaf: 7, subleaf: 0, result: structured},
  ...[1, 2, 0x10000, 0xffffffff].map(subleaf => ({leaf: 7, subleaf, result: zero})),
  {leaf: 0x0d, subleaf: 0, result: xstate},
  {leaf: 0x0d, subleaf: 1, result: zero},
  {leaf: 0x0d, subleaf: 2, result: upper},
  // no optimized/compacted/supervisor saves, xgetbv1, or further extended components.
  ...[3, 4, 9, 17, 18, 31, 32, 63, 64, 0xffffffff]
    .map(subleaf => ({leaf: 0x0d, subleaf, result: zero})),
  ...[0, 0xffffffff].map(subleaf => ({leaf: 0x80000000, subleaf, result: extendedLimit})),
  ...[0, 0xffffffff].map(subleaf => ({leaf: 0x80000001, subleaf, result: extendedFeatures})),
  // unknown leaves and structured subleaves are zero by software profile policy.
  ...[2, 6, 8, 0x7fffffff, 0x80000002, 0xffffffff, 0xdeadbeef]
    .map(leaf => ({leaf, subleaf: 0xa55aa55a, result: zero})),
];
