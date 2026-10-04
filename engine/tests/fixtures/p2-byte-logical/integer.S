.intel_syntax noprefix
.code32
.text

.macro fixture name
.globl \name
.type \name, @function
\name:
.endm
.macro end_fixture name
.size \name, .-\name
.endm

fixture and_rm_reg
    and al, cl
end_fixture and_rm_reg
fixture and_reg_rm
    .byte 0x22, 0xc4
end_fixture and_reg_rm
fixture and_al_imm
    and al, 0x80
end_fixture and_al_imm
fixture and_modrm_imm
    and bh, 0xff
end_fixture and_modrm_imm

fixture or_rm_reg
    or ah, ch
end_fixture or_rm_reg
fixture or_reg_rm
    .byte 0x0a, 0xe0
end_fixture or_reg_rm
fixture or_al_imm
    or al, 0x80
end_fixture or_al_imm
fixture or_modrm_imm
    or bh, 0xff
end_fixture or_modrm_imm

fixture xor_rm_reg
    xor ch, ch
end_fixture xor_rm_reg
fixture xor_reg_rm
    .byte 0x32, 0xc4
end_fixture xor_reg_rm
fixture xor_al_imm
    xor al, 0x80
end_fixture xor_al_imm
fixture xor_modrm_imm
    xor bh, 0xff
end_fixture xor_modrm_imm
