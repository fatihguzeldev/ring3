// result order follows the x86 state record: eax, ecx, edx, ebx.
const vendor = [7, 0x20656e69, 0x676e4533, 0x676e6952];
const features = [0x600, 0x00c00201, 0x07808111, 0];
const structured = [0, 0, 0, 0x108];
const extendedLimit = [0x80000001, 0, 0, 0];
const extendedFeatures = [0, 0x20, 0, 0];
const zero = [0, 0, 0, 0];

export const queries = [
  ...[0, 0xffffffff].map(subleaf => ({leaf: 0, subleaf, result: vendor})),
  ...[0, 0xffffffff].map(subleaf => ({leaf: 1, subleaf, result: features})),
  {leaf: 7, subleaf: 0, result: structured},
  ...[1, 2, 0x10000, 0xffffffff].map(subleaf => ({leaf: 7, subleaf, result: zero})),
  ...[0, 0xffffffff].map(subleaf => ({leaf: 0x80000000, subleaf, result: extendedLimit})),
  ...[0, 0xffffffff].map(subleaf => ({leaf: 0x80000001, subleaf, result: extendedFeatures})),
  // unknown leaves and structured subleaves are zero by software profile policy.
  ...[2, 6, 8, 0x7fffffff, 0x80000002, 0xffffffff, 0xdeadbeef]
    .map(leaf => ({leaf, subleaf: 0xa55aa55a, result: zero})),
];
