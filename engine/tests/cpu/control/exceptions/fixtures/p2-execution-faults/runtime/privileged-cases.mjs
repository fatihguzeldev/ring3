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
  ['wbinvd', [0x0f, 0x09]],
  ['rdmsr', [0x0f, 0x32]],
  ['wrmsr', [0x0f, 0x30]],
  ['cpl3-iopl0-pvi0-cli', [0xfa]],
  ['cpl3-iopl0-pvi0-sti', [0xfb]],
];
for (const opcode of [0x20, 0x22]) for (const control of [0, 2, 3, 4]) {
  legacy.push([`mov-${opcode.toString(16)}-cr${control}`, [0x0f, opcode, 0xc7 | control << 3]]);
  legacy.push([`mov-${opcode.toString(16)}-cr${control}-ignored-mod`, [0x0f, opcode, control << 3]]);
}
for (const opcode of [0x21, 0x23]) for (let debug = 0; debug < 8; debug++) {
  const modes = debug === 4 || debug === 5 ? [0, 0x40, 0x80, 0xc0] : [0xc0];
  for (const mode of modes) legacy.push([
    `mov-${opcode.toString(16)}-dr${debug}-mod${mode.toString(16)}`,
    [0x0f, opcode, mode | debug << 3 | (debug + 3) % 8],
  ]);
}
const memory = legacy.filter(([name]) => /^(invlpg|lgdt|lidt)$|-memory$/.test(name));
const rows = [...legacy, ...memory.flatMap(([name, bytes]) => [
  [`${name}-word`, [0x66, ...bytes]],
  [`${name}-address16`, [0x67, ...bytes]],
  [`${name}-fs`, [0x64, ...bytes]],
  [`${name}-ss`, [0x36, ...bytes]],
]), ...[0x66, 0x67, 0x64, 0x36].map(prefix => [
  `wbinvd-prefix-${prefix.toString(16)}`, [prefix, 0x0f, 0x09],
])];
export const positive = rows.flatMap(([name, bytes]) => [
  {name, bytes: Buffer.from(bytes), reason: 11, baseVersion: 6},
  {name: `invalid-lock-${name}`, bytes: Buffer.from([0xf0, ...bytes]), reason: 20, baseVersion: 1},
]);
export const negatives = legacy.flatMap(([name, bytes]) => [0xf2, 0xf3]
  .filter(prefix => name !== 'wbinvd' || prefix !== 0xf3)
  .map(prefix => ({
    name: `reserved-repeat-${prefix.toString(16)}-${name}`, bytes: Buffer.from([prefix, ...bytes]),
  })));
for (const [name, bytes] of [
  ['distinct-wbnoinvd', [0xf3, 0x0f, 0x09]],
  ['environment-dependent-in', [0xe4, 0x80]],
  ['environment-dependent-out', [0xe6, 0x80]],
  ['valid-unsupported-int80', [0xcd, 0x80]],
]) negatives.push({name, bytes: Buffer.from(bytes)});
export const claim = 'bounded CPL3 privileged GP0 whitelist and explicit IOPL0/PVI0/GD0/DE0 user environment; strict public preparation, invalidLOCK priority, reservedREP refusal and full CPU/arena preservation; present MSR instructions and PCE0 RDPMC fault at CPL3; IO environment remains unsupported';
