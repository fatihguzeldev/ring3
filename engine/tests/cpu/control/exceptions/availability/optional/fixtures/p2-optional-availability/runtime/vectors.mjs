// literal byte oracles are independent of the production availability predicate.
export const unavailable = [
  {
    "code": "Ptwrite_rm32",
    "hex": "f30faee0"
  },
  {
    "code": "Xsaveopt_mem",
    "hex": "0fae30"
  },
  {
    "code": "Clwb_m8",
    "hex": "660fae30"
  },
  {
    "code": "Tpause_r32",
    "hex": "660faef0"
  },
  {
    "code": "Umonitor_r16",
    "hex": "67f30faef0"
  },
  {
    "code": "Umonitor_r32",
    "hex": "f30faef0"
  },
  {
    "code": "Umwait_r32",
    "hex": "f20faef0"
  },
  {
    "code": "Clflushopt_m8",
    "hex": "660fae38"
  },
  {
    "code": "Xsavec_mem",
    "hex": "0fc720"
  },
  {
    "code": "Sha1nexte_xmm_xmmm128",
    "hex": "0f38c8c1"
  },
  {
    "code": "Sha1msg1_xmm_xmmm128",
    "hex": "0f38c9c1"
  },
  {
    "code": "Sha1msg2_xmm_xmmm128",
    "hex": "0f38cac1"
  },
  {
    "code": "Sha256rnds2_xmm_xmmm128",
    "hex": "0f38cbc1"
  },
  {
    "code": "Sha256msg1_xmm_xmmm128",
    "hex": "0f38ccc1"
  },
  {
    "code": "Sha256msg2_xmm_xmmm128",
    "hex": "0f38cdc1"
  },
  {
    "code": "Gf2p8mulb_xmm_xmmm128",
    "hex": "660f38cfc1"
  },
  {
    "code": "VEX_Vgf2p8mulb_xmm_xmm_xmmm128",
    "hex": "c4e271cfc2"
  },
  {
    "code": "VEX_Vgf2p8mulb_ymm_ymm_ymmm256",
    "hex": "c4e275cfc2"
  },
  {
    "code": "Adcx_r32_rm32",
    "hex": "660f38f6c1"
  },
  {
    "code": "Adox_r32_rm32",
    "hex": "f30f38f6c1"
  },
  {
    "code": "Movdir64b_r16_m512",
    "hex": "67660f38f808"
  },
  {
    "code": "Movdir64b_r32_m512",
    "hex": "660f38f808"
  },
  {
    "code": "Movdiri_m32_r32",
    "hex": "0f38f908"
  },
  {
    "code": "Sha1rnds4_xmm_xmmm128_imm8",
    "hex": "0f3accc100"
  },
  {
    "code": "Gf2p8affineqb_xmm_xmmm128_imm8",
    "hex": "660f3acec100"
  },
  {
    "code": "VEX_Vgf2p8affineqb_xmm_xmm_xmmm128_imm8",
    "hex": "c4e3f1cec200"
  },
  {
    "code": "VEX_Vgf2p8affineqb_ymm_ymm_ymmm256_imm8",
    "hex": "c4e3f5cec200"
  },
  {
    "code": "Gf2p8affineinvqb_xmm_xmmm128_imm8",
    "hex": "660f3acfc100"
  },
  {
    "code": "VEX_Vgf2p8affineinvqb_xmm_xmm_xmmm128_imm8",
    "hex": "c4e3f1cfc200"
  },
  {
    "code": "VEX_Vgf2p8affineinvqb_ymm_ymm_ymmm256_imm8",
    "hex": "c4e3f5cfc200"
  },
  {
    "code": "Serialize",
    "hex": "0f01e8"
  }
];
export const memoryForms = [
  {code: 'Adcx_r32_rm32', hex: '660f38f601', variant: 'memory'},
  {code: 'Ptwrite_rm32', hex: 'f30fae20', variant: 'memory'},
  {code: 'Sha1msg1_xmm_xmmm128', hex: '0f38c901', variant: 'memory'},
  {code: 'VEX_Vgf2p8affineqb_ymm_ymm_ymmm256_imm8', hex: 'c4e3f5ce0200', variant: 'memory'},
];
export const nops = [
  {code: 'Cldemote_m8', hex: '0f1c05ffffffff'},
  {code: 'Rdsspd_r32', hex: 'f30f1ec8'},
  {code: 'Cldemote_m8', hex: '360f1c0424', variant: 'limited SS'},
];
export const repairs = [
  {code: 'Adcx_r32_rm32', hex: '660f38f6c1', reason: 20},
  {code: 'VEX_Vgf2p8affineqb_ymm_ymm_ymmm256_imm8', hex: 'c4e3f5cec200', reason: 20},
  {code: 'Cldemote_m8', hex: '0f1c05ffffffff', reason: 1},
];
export const refusals = [
  {label: 'duplicate mandatory66 ADCX stays unsupported', hex: '66660f38f6c1'},
  {label: 'duplicate mandatoryF3 ADOX stays unsupported', hex: 'f3f30f38f6c1'},
  {label: 'unrelated REPNE RSM stays unsupported', hex: 'f20faa'},
  {label: 'unrelated INT80 stays unsupported', hex: 'cd80'},
];
export const invalidLocks = [
  {label: 'LOCK CLDEMOTE stays invalid', hex: 'f00f1c00'},
  {label: 'LOCK RDSSPD stays invalid', hex: 'f0f30f1ec8'},
];
export const cpuidQueries = [
  {leaf: 7, subleaf: 0, output: [0, 0x128, 0x400000, 0]},
  {leaf: 13, subleaf: 1, output: [0, 0, 0, 0]},
  {leaf: 20, subleaf: 0, output: [0, 0, 0, 0]},
];
