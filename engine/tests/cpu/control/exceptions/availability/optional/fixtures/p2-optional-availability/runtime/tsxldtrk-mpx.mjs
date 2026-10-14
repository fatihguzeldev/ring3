// literal finite cases; no production availability predicate supplies an oracle.
export const tsxUnavailable = [
  {code: 'Xsusldtrk', hex: 'f20f01e8'},
  {code: 'Xresldtrk', hex: 'f20f01e9'},
];

// iced DecoderOptions::NONE exposes these canonical disabled-mpx encodings as reserved nop Codes.
// architectural bnd operands are named here only to identify the original bytes.
export const mpxNops = [
  {code: 'Reservednop_rm16_r16_0F1A', hex: '660f1ac0', variant: 'BND0 <- BND0'},
  {code: 'Reservednop_rm16_r16_0F1A', hex: '660f1ac1', variant: 'BND0 <- BND1', schedules: true},
  {code: 'Reservednop_rm16_r16_0F1A', hex: '660f1ac2', variant: 'BND0 <- BND2'},
  {code: 'Reservednop_rm16_r16_0F1A', hex: '660f1ac3', variant: 'BND0 <- BND3'},
  {code: 'Reservednop_rm16_r16_0F1A', hex: '660f1ac8', variant: 'BND1 <- BND0'},
  {code: 'Reservednop_rm16_r16_0F1A', hex: '660f1ac9', variant: 'BND1 <- BND1'},
  {code: 'Reservednop_rm16_r16_0F1A', hex: '660f1aca', variant: 'BND1 <- BND2'},
  {code: 'Reservednop_rm16_r16_0F1A', hex: '660f1acb', variant: 'BND1 <- BND3'},
  {code: 'Reservednop_rm16_r16_0F1A', hex: '660f1ad0', variant: 'BND2 <- BND0'},
  {code: 'Reservednop_rm16_r16_0F1A', hex: '660f1ad1', variant: 'BND2 <- BND1'},
  {code: 'Reservednop_rm16_r16_0F1A', hex: '660f1ad2', variant: 'BND2 <- BND2'},
  {code: 'Reservednop_rm16_r16_0F1A', hex: '660f1ad3', variant: 'BND2 <- BND3'},
  {code: 'Reservednop_rm16_r16_0F1A', hex: '660f1ad8', variant: 'BND3 <- BND0'},
  {code: 'Reservednop_rm16_r16_0F1A', hex: '660f1ad9', variant: 'BND3 <- BND1'},
  {code: 'Reservednop_rm16_r16_0F1A', hex: '660f1ada', variant: 'BND3 <- BND2'},
  {code: 'Reservednop_rm16_r16_0F1A', hex: '660f1adb', variant: 'BND3 <- BND3'},
  {code: 'Reservednop_rm16_r16_0F1B', hex: '660f1bc0', variant: 'BND0 -> BND0'},
  {code: 'Reservednop_rm16_r16_0F1B', hex: '660f1bc1', variant: 'BND0 -> BND1', schedules: true},
  {code: 'Reservednop_rm16_r16_0F1B', hex: '660f1bc2', variant: 'BND0 -> BND2'},
  {code: 'Reservednop_rm16_r16_0F1B', hex: '660f1bc3', variant: 'BND0 -> BND3'},
  {code: 'Reservednop_rm16_r16_0F1B', hex: '660f1bc8', variant: 'BND1 -> BND0'},
  {code: 'Reservednop_rm16_r16_0F1B', hex: '660f1bc9', variant: 'BND1 -> BND1'},
  {code: 'Reservednop_rm16_r16_0F1B', hex: '660f1bca', variant: 'BND1 -> BND2'},
  {code: 'Reservednop_rm16_r16_0F1B', hex: '660f1bcb', variant: 'BND1 -> BND3'},
  {code: 'Reservednop_rm16_r16_0F1B', hex: '660f1bd0', variant: 'BND2 -> BND0'},
  {code: 'Reservednop_rm16_r16_0F1B', hex: '660f1bd1', variant: 'BND2 -> BND1'},
  {code: 'Reservednop_rm16_r16_0F1B', hex: '660f1bd2', variant: 'BND2 -> BND2'},
  {code: 'Reservednop_rm16_r16_0F1B', hex: '660f1bd3', variant: 'BND2 -> BND3'},
  {code: 'Reservednop_rm16_r16_0F1B', hex: '660f1bd8', variant: 'BND3 -> BND0'},
  {code: 'Reservednop_rm16_r16_0F1B', hex: '660f1bd9', variant: 'BND3 -> BND1'},
  {code: 'Reservednop_rm16_r16_0F1B', hex: '660f1bda', variant: 'BND3 -> BND2'},
  {code: 'Reservednop_rm16_r16_0F1B', hex: '660f1bdb', variant: 'BND3 -> BND3'},
];

export const boundedRepairs = [
  {code: 'Xsusldtrk', hex: 'f20f01e8', reason: 20},
  {code: 'Xresldtrk', hex: 'f20f01e9', reason: 20},
  {code: 'Reservednop_rm16_r16_0F1A', hex: '660f1ac1', reason: 16},
  {code: 'Reservednop_rm16_r16_0F1B', hex: '660f1bc1', reason: 16},
];
export const boundedRefusals = [
  {label: 'duplicate mandatory F2 XSUSLDTRK retains host refusal', hex: 'f2f20f01e8'},
  {label: 'duplicate mandatory F2 XRESLDTRK retains host refusal', hex: 'f2f20f01e9'},
  {label: 'duplicate 66 BNDMOV 1A retains host refusal', hex: '66660f1ac1'},
  {label: 'duplicate 66 BNDMOV 1B retains host refusal', hex: '66660f1bc1'},
];
export const boundedInvalidLocks = [
  {label: 'LOCK XSUSLDTRK is invalid', hex: 'f0f20f01e8'},
  {label: 'LOCK XRESLDTRK is invalid', hex: 'f0f20f01e9'},
  {label: 'LOCK canonical register BNDMOV 1A is invalid', hex: 'f0660f1ac1'},
  {label: 'LOCK canonical register BNDMOV 1B is invalid', hex: 'f0660f1bc1'},
];

export const boundedCompatibilityNops = [
  {label: 'extra 67 BNDMOV 1A is a disabled-MPX NOP', hex: '67660f1ac1'},
  {label: 'extra 67 BNDMOV 1B is a disabled-MPX NOP', hex: '67660f1bc1'},
  {label: 'memory BNDMOV 1A is a disabled-MPX NOP', hex: '660f1a00'},
  {label: 'memory BNDMOV 1B is a disabled-MPX NOP', hex: '660f1b00'},
];
