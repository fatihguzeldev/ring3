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

fixture cmp_register
    cmp byte ptr [esi], ah
end_fixture cmp_register
fixture cmp_immediate
    cmp byte ptr [esi], 0x80
end_fixture cmp_immediate
fixture test_register
    test byte ptr [esi], al
end_fixture test_register
fixture test_immediate
    test byte ptr [esi], 0x80
end_fixture test_immediate
