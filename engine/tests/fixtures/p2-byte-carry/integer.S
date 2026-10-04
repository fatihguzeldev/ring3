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

fixture adc_rm_reg
    adc al, cl
end_fixture adc_rm_reg
fixture adc_reg_rm
    .byte 0x12, 0xc4
end_fixture adc_reg_rm
fixture adc_al_imm
    adc al, 0x80
end_fixture adc_al_imm
fixture adc_modrm_imm
    adc bh, 0xff
end_fixture adc_modrm_imm

fixture sbb_rm_reg
    sbb ah, ch
end_fixture sbb_rm_reg
fixture sbb_reg_rm
    .byte 0x1a, 0xe0
end_fixture sbb_reg_rm
fixture sbb_al_imm
    sbb al, 0x80
end_fixture sbb_al_imm
fixture sbb_modrm_imm
    sbb bh, 0xff
end_fixture sbb_modrm_imm
