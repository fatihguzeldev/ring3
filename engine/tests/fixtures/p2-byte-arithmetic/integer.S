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

fixture add_rm_reg
    add al, cl
end_fixture add_rm_reg
fixture add_reg_rm
    .byte 0x02, 0xc4
end_fixture add_reg_rm
fixture add_al_imm
    add al, 0x80
end_fixture add_al_imm
fixture add_modrm_imm
    add bh, 0xff
end_fixture add_modrm_imm

fixture sub_rm_reg
    sub ah, ch
end_fixture sub_rm_reg
fixture sub_reg_rm
    .byte 0x2a, 0xe0
end_fixture sub_reg_rm
fixture sub_al_imm
    sub al, 0x80
end_fixture sub_al_imm
fixture sub_modrm_imm
    sub bh, 0xff
end_fixture sub_modrm_imm
