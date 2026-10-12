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

fixture inc_al
    inc al
end_fixture inc_al
fixture dec_ah
    dec ah
end_fixture dec_ah
fixture neg_cl
    neg cl
end_fixture neg_cl
fixture not_bh
    not bh
end_fixture not_bh
