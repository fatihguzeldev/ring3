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

fixture shl_one
    shl al, 1
end_fixture shl_one
fixture shl_imm
    shl ah, 0xff
end_fixture shl_imm
fixture shl_cl
    shl ch, cl
end_fixture shl_cl
fixture shr_one
    shr al, 1
end_fixture shr_one
fixture shr_imm
    shr ah, 0xff
end_fixture shr_imm
fixture shr_cl
    shr ch, cl
end_fixture shr_cl
fixture sar_one
    sar al, 1
end_fixture sar_one
fixture sar_imm
    sar ah, 0xff
end_fixture sar_imm
fixture sar_cl
    sar ch, cl
end_fixture sar_cl
