const legacy = [
  ['clts', [0x0f, 0x06]],
  ['invlpg', [0x0f, 0x01, 0x38]],
  ['lgdt', [0x0f, 0x01, 0x10]],
  ['lidt', [0x0f, 0x01, 0x18]],
  ['lldt-register', [0x0f, 0x00, 0xd0]],
  ['lldt-memory', [0x0f, 0x00, 0x10]],
  ['ltr-register', [0x0f, 0x00, 0xd8]],
  ['ltr-memory', [0x0f, 0x00, 0x18]],
  ['lmsw-register', [0x0f, 0x01, 0xf0]],
  ['lmsw-memory', [0x0f, 0x01, 0x30]],
  ['invd', [0x0f, 0x08]],
];
for (const opcode of [0x20, 0x22]) for (const control of [0, 2, 3, 4]) {
  legacy.push([`mov-${opcode.toString(16)}-cr${control}`, [0x0f, opcode, 0xc7 | control << 3]]);
  legacy.push([`mov-${opcode.toString(16)}-cr${control}-ignored-mod`, [0x0f, opcode, control << 3]]);
}
const memory = legacy.filter(([name]) => /^(invlpg|lgdt|lidt)$|-memory$/.test(name));
const rows = [...legacy, ...memory.flatMap(([name, bytes]) => [
  [`${name}-word`, [0x66, ...bytes]],
  [`${name}-address16`, [0x67, ...bytes]],
  [`${name}-fs`, [0x64, ...bytes]],
  [`${name}-ss`, [0x36, ...bytes]],
])];
export const positive = rows.flatMap(([name, bytes]) => [
  {name, bytes: Buffer.from(bytes), reason: 11, baseVersion: 6},
  {name: `invalid-lock-${name}`, bytes: Buffer.from([0xf0, ...bytes]), reason: 20, baseVersion: 1},
]);
export const negatives = legacy.flatMap(([name, bytes]) => [0xf2, 0xf3].map(prefix => ({
  name: `reserved-repeat-${prefix.toString(16)}-${name}`, bytes: Buffer.from([prefix, ...bytes]),
})));
for (const [name, bytes] of [
  ['unadvertised-rdmsr', [0x0f, 0x32]],
  ['unadvertised-wrmsr', [0x0f, 0x30]],
  ['unclassified-wbinvd', [0x0f, 0x09]],
  ['distinct-wbnoinvd', [0xf3, 0x0f, 0x09]],
  ['debug-environment-mov-dr0', [0x0f, 0x21, 0xc0]],
  ['debug-environment-mov-dr4', [0x0f, 0x23, 0xe0]],
  ['environment-dependent-cli', [0xfa]],
  ['environment-dependent-sti', [0xfb]],
  ['environment-dependent-in', [0xe4, 0x80]],
  ['environment-dependent-out', [0xe6, 0x80]],
  ['valid-unsupported-int80', [0xcd, 0x80]],
]) negatives.push({name, bytes: Buffer.from(bytes)});
export const claim = 'bounded CPL3 privileged GP0 whitelist; strict public preparation, invalidLOCK priority, reservedREP refusal and full CPU/arena preservation; MSR/debug/IO environments remain unsupported';
